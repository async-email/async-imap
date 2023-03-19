use std::fmt;
use std::pin::Pin;

#[cfg(feature = "runtime-async-std")]
use async_std::io::{BufRead, Write, WriteExt};
use futures::stream::Stream;
use futures::task::{Context, Poll};
use futures::{io, ready};
use nom::Needed;
#[cfg(feature = "runtime-tokio")]
use tokio::io::{AsyncBufRead as BufRead, AsyncWrite as Write, AsyncWriteExt};

use crate::types::{Request, ResponseData};

/// Wraps a stream, and parses incoming data as imap server messages. Writes outgoing data
/// as imap client messages.
#[derive(Debug)]
pub struct ImapStream<R: BufRead + Write> {
    // TODO: write some buffering logic
    /// The underlying stream
    pub(crate) inner: R,
    /// Number of bytes the next decode operation needs if known.
    /// If the buffer contains less than this, it is a waste of time to try to parse it.
    /// If unknown, set it to 0, so decoding is always attempted.
    decode_needs: usize,
    /// Whether there is any more items to return from the stream.  This is set to true once
    /// all decodable data in the buffer is returned and the underlying stream is closed.
    closed: bool,
}

impl<R: BufRead + Write + Unpin> ImapStream<R> {
    /// Creates a new `ImapStream` based on the given `Read`er.
    pub fn new(inner: R) -> Self {
        ImapStream {
            inner,
            decode_needs: 0,
            closed: false,
        }
    }

    pub async fn encode(&mut self, msg: Request) -> Result<(), io::Error> {
        log::trace!(
            "encode: input: {:?}, {:?}",
            msg.0,
            std::str::from_utf8(&msg.1)
        );

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

    /// End-Of-File return value.
    ///
    /// Return the appropriate EOF value for the stream depending on whether there is still
    /// data in the buffer.  It is assumed that any remaining data in the buffer can not be
    /// decoded.
    fn stream_eof_value(&self) -> Option<io::Result<ResponseData>> {
        match self.buffer.used() {
            0 => None,
            _ => Some(Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "bytes remaining in stream",
            ))),
        }
    }
}

impl<R: BufRead + Write + Unpin> ImapStream<R> {
    fn maybe_decode(&mut self) -> io::Result<Option<ResponseData>> {
        if self.buffer.used() >= self.decode_needs {
            self.decode()
        } else {
            Ok(None)
        }
    }

    fn decode(&mut self) -> io::Result<Option<ResponseData>> {
        let block: Block<'static> = self.buffer.take_block();
        // Be aware, now self.buffer is invalid until block is returned or reset!

        let res = ResponseData::try_new_or_recover(block, |buf| {
            let buf = &buf[..self.buffer.used()];
            log::trace!("decode: input: {:?}", std::str::from_utf8(buf));
            match imap_proto::parser::parse_response(buf) {
                Ok((remaining, response)) => {
                    // TODO: figure out if we can use a minimum required size for a response.
                    self.decode_needs = 0;
                    self.buffer.reset_with_data(remaining);
                    Ok(response)
                }
                Err(nom::Err::Incomplete(Needed::Size(min))) => {
                    log::trace!("decode: incomplete data, need minimum {} bytes", min);
                    self.decode_needs = self.buffer.used() + usize::from(min);
                    Err(None)
                }
                Err(nom::Err::Incomplete(_)) => {
                    log::trace!("decode: incomplete data, need unknown number of bytes");
                    self.decode_needs = 0;
                    Err(None)
                }
                Err(other) => {
                    self.decode_needs = 0;
                    Err(Some(io::Error::new(
                        io::ErrorKind::Other,
                        format!("{:?} during parsing of {:?}", other, buf),
                    )))
                }
            }
        });
        match res {
            Ok(response) => Ok(Some(response)),
            Err((err, heads)) => {
                self.buffer.return_block(heads.raw);
                match err {
                    Some(err) => Err(err),
                    None => Ok(None),
                }
            }
        }
    }
}

impl<R: BufRead + Write + Unpin> Stream for ImapStream<R> {
    type Item = io::Result<ResponseData>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        if let Some(response) = this.maybe_decode()? {
            return Poll::Ready(Some(Ok(response)));
        }
        if this.closed {
            return Poll::Ready(this.stream_eof_value());
        }
        loop {
            this.buffer.ensure_capacity(this.decode_needs)?;
            let buf = this.buffer.free_as_mut_slice();

            #[cfg(feature = "runtime-async-std")]
            let num_bytes_read = ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;

            #[cfg(feature = "runtime-tokio")]
            let num_bytes_read = {
                let buf = &mut tokio::io::ReadBuf::new(buf);
                let start = buf.filled().len();
                ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
                buf.filled().len() - start
            };

            if num_bytes_read == 0 {
                this.closed = true;
                return Poll::Ready(this.stream_eof_value());
            }
            this.buffer.extend_used(num_bytes_read);
            if let Some(response) = this.maybe_decode()? {
                return Poll::Ready(Some(Ok(response)));
            }
        }
    }
}
