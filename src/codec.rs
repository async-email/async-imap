use std::ops::Deref;
use std::pin::Pin;

use async_std::io::{self, Read, Write};
use async_std::prelude::*;
use async_std::stream::Stream;
use async_std::sync::Arc;
use bytes::{BufMut, BytesMut};
use futures::task::{Context, Poll};
use imap_proto::{RequestId, Response};
use nom::Needed;

use byte_pool::{Block, BytePool};

const INITIAL_CAPACITY: usize = 1024 * 4;

lazy_static::lazy_static! {
    static ref POOL: Arc<BytePool> = Arc::new(BytePool::new());
}

#[derive(Debug)]
pub struct ResponseStream<R: Read> {
    // TODO: write some buffering logic
    pub(crate) inner: R,
    buffer: Block<'static>,
    /// Position of valid read data into buffer.
    current: usize,
    decode_needs: usize,
}

enum DecodeResult {
    Some(ResponseData),
    None(Block<'static>),
}

impl<R: Read + Write + Unpin> ResponseStream<R> {
    /// Creates a new `ResponseStream` based on the given `Read`er.
    pub fn new(inner: R) -> Self {
        ResponseStream {
            inner,
            buffer: POOL.alloc(INITIAL_CAPACITY),
            current: 0,
            decode_needs: 0,
        }
    }

    pub async fn encode(&mut self, msg: Request) -> Result<(), io::Error> {
        if let Some(tag) = msg.0 {
            self.inner.write_all(tag.as_bytes()).await?;
            self.inner.write(b" ").await?;
        }
        self.inner.write_all(&msg.1).await?;
        self.inner.write_all(b"\r\n").await?;

        Ok(())
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Unpin> ResponseStream<R> {
    fn decode(&mut self, buf: Block<'static>, n: usize) -> io::Result<DecodeResult> {
        if self.decode_needs > n {
            return Ok(DecodeResult::None(buf));
        }

        let res = ResponseData::try_new(buf, |buf| {
            match imap_proto::parse_response(&buf[..n]) {
                Ok((remaining, response)) => {
                    // TODO: figure out if we can shrink to the minimum required size.
                    self.decode_needs = 0;
                    Ok(response)
                }
                Err(nom::Err::Incomplete(Needed::Size(min))) => {
                    self.decode_needs = min;
                    Err(None)
                }
                Err(nom::Err::Incomplete(_)) => Err(None),
                Err(err) => Err(Some(io::Error::new(
                    io::ErrorKind::Other,
                    format!("{:?} during parsing of {:?}", err, buf),
                ))),
            }
        });

        match res {
            Ok(res) => Ok(DecodeResult::Some(res)),
            Err(rental::RentalError(err, buf)) => match err {
                Some(err) => Err(err),
                None => Ok(DecodeResult::None(buf)),
            },
        }
    }
}

impl<R: Read + Unpin> Stream for ResponseStream<R> {
    type Item = io::Result<ResponseData>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        let buffer = std::mem::replace(&mut this.buffer, POOL.alloc(INITIAL_CAPACITY));

        let mut buffer = match this.decode(buffer, this.current)? {
            DecodeResult::Some(item) => {
                return Poll::Ready(Some(Ok(item)));
            }
            DecodeResult::None(buffer) => buffer,
        };

        loop {
            if this.current >= buffer.capacity() {
                // TODO: add max size
                buffer.realloc(buffer.capacity() + 1024);
            }

            let n = futures::ready!(
                Pin::new(&mut this.inner).poll_read(cx, &mut buffer[this.current..])
            )?;

            this.current += n;

            match this.decode(buffer, n)? {
                DecodeResult::Some(item) => return Poll::Ready(Some(Ok(item))),
                DecodeResult::None(buf) => {
                    buffer = buf;

                    if this.buffer.is_empty() {
                        // put back the buffer to reuse it
                        std::mem::replace(&mut this.buffer, buffer);

                        return Poll::Ready(None);
                    } else if n == 0 {
                        // put back the buffer to reuse it
                        std::mem::replace(&mut this.buffer, buffer);

                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "bytes remaining in stream",
                        )
                        .into())));
                    }
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ImapCodec {
    decode_need_message_bytes: usize,
}

rental! {
    pub mod rents {
        use super::*;

        #[rental(debug, covariant)]
        pub struct ResponseData {
            raw: Block<'static>,
            response: Response<'raw>,
        }
    }
}

pub use rents::ResponseData;

impl std::cmp::PartialEq for ResponseData {
    fn eq(&self, other: &Self) -> bool {
        self.parsed() == other.parsed()
    }
}

impl std::cmp::Eq for ResponseData {}

impl ResponseData {
    pub fn request_id(&self) -> Option<&RequestId> {
        match self.suffix() {
            Response::Done { ref tag, .. } => Some(tag),
            _ => None,
        }
    }

    pub fn parsed(&self) -> &Response<'_> {
        self.suffix()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Request(pub Option<RequestId>, pub Vec<u8>);

#[derive(Debug)]
pub struct IdGenerator {
    next: u64,
}

impl IdGenerator {
    pub fn new() -> Self {
        Self { next: 0 }
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl Iterator for IdGenerator {
    type Item = RequestId;
    fn next(&mut self) -> Option<Self::Item> {
        self.next += 1;
        Some(RequestId(format!("A{:04}", self.next % 10_000)))
    }
}
