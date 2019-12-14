use std::pin::Pin;

use async_std::io::{self, Read, Write};
use async_std::prelude::*;
use async_std::stream::Stream;
use async_std::sync::Arc;
use byte_pool::{Block, BytePool};
use futures::task::{Context, Poll};
use nom::Needed;

use crate::types::{Request, ResponseData};

const INITIAL_CAPACITY: usize = 1024 * 4;
const MAX_CAPACITY: usize = 512 * 1024 * 1024; // 512 MiB

lazy_static::lazy_static! {
    pub(crate) static ref POOL: Arc<BytePool> = Arc::new(BytePool::new());
}

#[derive(Debug)]
pub struct ImapStream<R: Read> {
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

impl<R: Read + Write + Unpin> ImapStream<R> {
    /// Creates a new `ImapStream` based on the given `Read`er.
    pub fn new(inner: R) -> Self {
        ImapStream {
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

impl<R: Read + Unpin> ImapStream<R> {
    fn decode(&mut self, buf: Block<'static>, n: usize) -> io::Result<DecodeResult> {
        if self.decode_needs > n {
            return Ok(DecodeResult::None(buf));
        }

        let res = ResponseData::try_new(buf, |buf| {
            match imap_proto::parse_response(&buf[..n]) {
                Ok((_remaining, response)) => {
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
                    format!("{:?} during parsing of {:?}", err, &buf[..n]),
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

impl<R: Read + Unpin> Stream for ImapStream<R> {
    type Item = io::Result<ResponseData>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        let mut n = this.current;
        this.current = 0;
        let buffer = std::mem::replace(&mut this.buffer, POOL.alloc(INITIAL_CAPACITY));

        let mut buffer = match this.decode(buffer, n)? {
            DecodeResult::Some(item) => {
                return Poll::Ready(Some(Ok(item)));
            }
            DecodeResult::None(buffer) => buffer,
        };

        loop {
            if n >= buffer.capacity() {
                if buffer.capacity() + 1024 < MAX_CAPACITY {
                    buffer.realloc(buffer.capacity() + 1024);
                } else {
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::Other,
                        "incoming data too large",
                    ))));
                }
            }

            n += futures::ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buffer[n..]))?;

            match this.decode(buffer, n)? {
                DecodeResult::Some(item) => return Poll::Ready(Some(Ok(item))),
                DecodeResult::None(buf) => {
                    buffer = buf;

                    if this.buffer.is_empty() {
                        // put back the buffer to reuse it
                        std::mem::replace(&mut this.buffer, buffer);
                        this.current = n;

                        return Poll::Ready(None);
                    } else if n == 0 {
                        // put back the buffer to reuse it
                        std::mem::replace(&mut this.buffer, buffer);
                        this.current = n;

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
