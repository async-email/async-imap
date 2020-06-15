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
    current: Position,
    /// How many bytes do we need to finishe the currrent element that is being decoded.
    decode_needs: usize,
    /// Whether we should attempt to decode whatever is currently inside the buffer.
    /// False indicates that we know for certain that the buffer is incomplete.
    initial_decode: bool,
}

/// A semantically explicit slice of a buffer.
#[derive(Eq, PartialEq, Debug, Copy, Clone)]
struct Position {
    start: usize,
    end: usize,
}

impl Position {
    const ZERO: Position = Position { start: 0, end: 0 };

    const fn new(start: usize, end: usize) -> Position {
        Position { start, end }
    }
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
            current: Position::ZERO,
            decode_needs: 0,
            initial_decode: false, // buffer is empty initially, nothing to decode
        }
    }

    pub async fn encode(&mut self, msg: Request) -> Result<(), io::Error> {
        log::trace!("encode: input: {:?}", msg);

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
        log::trace!("decode: input: {:?}", std::str::from_utf8(&buf[start..end]));

        let mut rest = None;
        let mut used = 0;
        let res = ResponseData::try_new(buf, |buf| {
            match imap_proto::Response::from_bytes(&buf[start..end]) {
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
                    log::trace!("decode: incomplete data, need minimum {} bytes", min);
                    self.decode_needs = min;
                    Err(None)
                }
                Err(nom::Err::Incomplete(_)) => {
                    log::trace!("decode: incomplete data, need unknown number of bytes");
                    Err(None)
                }
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
        // The `poll_next` method must strive to be as idempotent as possible if the underlying
        // future/stream is not yet ready to produce results. It means that we must be careful
        // to persist the state of polling between calls to `poll_next`, specifically,
        // we must always restore the buffer and the current position before any `return`s.

        let this = &mut *self;

        let mut n = std::mem::replace(&mut this.current, Position::ZERO);
        let buffer = std::mem::replace(&mut this.buffer, POOL.alloc(INITIAL_CAPACITY));

        let mut buffer = if (n.end - n.start) > 0 && this.initial_decode {
            match this.decode(buffer, n.start, n.end)? {
                DecodeResult::Some {
                    response,
                    buffer,
                    used,
                } => {
                    // initial_decode is still true
                    std::mem::replace(&mut this.buffer, buffer);
                    this.current = Position::new(0, used);
                    return Poll::Ready(Some(Ok(response)));
                }
                DecodeResult::None(buffer) => buffer,
            }
        } else {
            buffer
        };

        loop {
            if (n.end - n.start) + this.decode_needs >= buffer.capacity() {
                if buffer.capacity() + this.decode_needs < MAX_CAPACITY {
                    buffer.realloc(buffer.capacity() + this.decode_needs);
                } else {
                    std::mem::replace(&mut this.buffer, buffer);
                    this.current = n;
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::Other,
                        "incoming data too large",
                    ))));
                }
            }

            let bytes_read = match Pin::new(&mut this.inner).poll_read(cx, &mut buffer[n.end..]) {
                Poll::Ready(result) => result?,
                Poll::Pending => {
                    // if we're here, it means that we need more data but there is none yet,
                    // so no decoding attempts are necessary until we get more data
                    this.initial_decode = false;

                    std::mem::replace(&mut this.buffer, buffer);
                    this.current = n;
                    return Poll::Pending;
                }
            };
            n.end += bytes_read;

            match this.decode(buffer, n.start, n.end)? {
                DecodeResult::Some {
                    response,
                    buffer,
                    used,
                } => {
                    // current buffer might now contain more data inside, so we need to attempt
                    // to decode it next time
                    this.initial_decode = true;

                    std::mem::replace(&mut this.buffer, buffer);
                    this.current = Position::new(0, used);
                    return Poll::Ready(Some(Ok(response)));
                }
                DecodeResult::None(buf) => {
                    buffer = buf;

                    if this.buffer.is_empty() || n == Position::ZERO {
                        // "logical buffer" is empty, there is nothing to decode on the next step
                        this.initial_decode = false;

                        std::mem::replace(&mut this.buffer, buffer);
                        this.current = n;
                        return Poll::Ready(None);
                    } else if (n.end - n.start) == 0 {
                        // "logical buffer" is empty, there is nothing to decode on the next step
                        this.initial_decode = false;

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
