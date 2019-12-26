use std::fmt;
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
    /// The global buffer pool we use for storing incoming data.
    pub(crate) static ref POOL: Arc<BytePool> = Arc::new(BytePool::new());
}

/// Wraps a stream, and parses incoming data as imap server messages. Writes outgoing data
/// as imap client messages.
#[derive(Debug)]
pub struct ImapStream<R: Read + Write> {
    // TODO: write some buffering logic
    /// The underlying stream
    pub(crate) inner: R,
    /// Buffer for the already read, but not yet parsed data.
    buffer: Block<'static>,
    /// Position of valid read data into buffer.
    current: (usize, usize),
    /// How many bytes do we need to finishe the currrent element that is being decoded.
    decode_needs: usize,
}

enum DecodeResult {
    Some {
        /// The parsed response.
        response: ResponseData,
        /// Remaining data.
        buffer: Block<'static>,
        /// How many bytes are actually valid data in `buffer`.
        used: usize,
    },
    None(Block<'static>),
}

impl fmt::Debug for DecodeResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeResult::Some {
                response,
                buffer,
                used,
            } => f
                .debug_struct("DecodeResult::Some")
                .field("response", response)
                .field("block", &buffer.len())
                .field("used", used)
                .finish(),
            DecodeResult::None(block) => write!(f, "DecodeResult::None({})", block.len()),
        }
    }
}

impl<R: Read + Write + Unpin> ImapStream<R> {
    /// Creates a new `ImapStream` based on the given `Read`er.
    pub fn new(inner: R) -> Self {
        ImapStream {
            inner,
            buffer: POOL.alloc(INITIAL_CAPACITY),
            current: (0, 0),
            decode_needs: 0,
        }
    }

    pub async fn encode(&mut self, msg: Request) -> Result<(), io::Error> {
        log::trace!("> {:?}", msg);

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

    /// Flushes the underlying stream.
    pub async fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush().await
    }

    pub fn as_mut(&mut self) -> &mut R {
        &mut self.inner
    }
}

impl<R: Read + Write + Unpin> ImapStream<R> {
    fn decode(
        &mut self,
        buf: Block<'static>,
        start: usize,
        end: usize,
    ) -> io::Result<DecodeResult> {
        log::trace!("< {:?}", std::str::from_utf8(&buf[start..end]));

        let mut rest = None;
        let mut used = 0;
        let res = ResponseData::try_new(buf, |buf| {
            match imap_proto::parse_response(&buf[start..end]) {
                Ok((remaining, response)) => {
                    // TODO: figure out if we can shrink to the minimum required size.
                    self.decode_needs = 0;

                    let mut buf = POOL.alloc(std::cmp::max(remaining.len(), INITIAL_CAPACITY));
                    buf[..remaining.len()].copy_from_slice(remaining);
                    used = remaining.len();

                    rest = Some(buf);

                    Ok(response)
                }
                Err(nom::Err::Incomplete(Needed::Size(min))) => {
                    self.decode_needs = min;
                    Err(None)
                }
                Err(nom::Err::Incomplete(_)) => Err(None),
                Err(err) => Err(Some(io::Error::new(
                    io::ErrorKind::Other,
                    format!("{:?} during parsing of {:?}", err, &buf[start..end]),
                ))),
            }
        });

        match res {
            Ok(response) => Ok(DecodeResult::Some {
                response,
                buffer: rest.unwrap(),
                used,
            }),
            Err(rental::RentalError(err, buf)) => match err {
                Some(err) => Err(err),
                None => Ok(DecodeResult::None(buf)),
            },
        }
    }
}

impl<R: Read + Write + Unpin> Stream for ImapStream<R> {
    type Item = io::Result<ResponseData>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;

        let mut n = this.current;
        this.current = (0, 0);
        let buffer = std::mem::replace(&mut this.buffer, POOL.alloc(INITIAL_CAPACITY));

        let mut buffer = if (n.1 - n.0) > 0 {
            match this.decode(buffer, n.0, n.1)? {
                DecodeResult::Some {
                    response,
                    buffer,
                    used,
                } => {
                    this.current = (0, used);
                    std::mem::replace(&mut this.buffer, buffer);

                    return Poll::Ready(Some(Ok(response)));
                }
                DecodeResult::None(buffer) => buffer,
            }
        } else {
            buffer
        };

        loop {
            if (n.1 - n.0) + this.decode_needs >= buffer.capacity() {
                if buffer.capacity() + this.decode_needs < MAX_CAPACITY {
                    buffer.realloc(buffer.capacity() + this.decode_needs);
                } else {
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::Other,
                        "incoming data too large",
                    ))));
                }
            }

            n.1 += futures::ready!(Pin::new(&mut this.inner).poll_read(cx, &mut buffer[n.1..]))?;

            match this.decode(buffer, n.0, n.1)? {
                DecodeResult::Some {
                    response,
                    buffer,
                    used,
                } => {
                    this.current = (0, used);
                    std::mem::replace(&mut this.buffer, buffer);

                    return Poll::Ready(Some(Ok(response)));
                }
                DecodeResult::None(buf) => {
                    buffer = buf;

                    if this.buffer.is_empty() || (n.0 == 0 && n.1 == 0) {
                        // put back the buffer to reuse it
                        std::mem::replace(&mut this.buffer, buffer);
                        this.current = n;

                        return Poll::Ready(None);
                    } else if (n.1 - n.0) == 0 {
                        // put back the buffer to reuse it
                        std::mem::replace(&mut this.buffer, buffer);
                        this.current = n;

                        return Poll::Ready(Some(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "bytes remaining in stream",
                        ))));
                    }
                }
            }
        }
    }
}
