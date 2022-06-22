use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

#[cfg(feature = "runtime-async-std")]
use async_std::io::{Read, Write, WriteExt};
use byte_pool::{Block, BytePool};
use futures::stream::Stream;
use futures::task::{Context, Poll};
use futures::{io, ready};
use nom::Needed;
use once_cell::sync::Lazy;
#[cfg(feature = "runtime-tokio")]
use tokio::io::{AsyncRead as Read, AsyncWrite as Write, AsyncWriteExt};

use crate::types::{Request, ResponseData};

/// The global buffer pool we use for storing incoming data.
pub(crate) static POOL: Lazy<Arc<BytePool>> = Lazy::new(|| Arc::new(BytePool::new()));

/// Wraps a stream, and parses incoming data as imap server messages. Writes outgoing data
/// as imap client messages.
#[derive(Debug)]
pub struct ImapStream<R: Read + Write> {
    // TODO: write some buffering logic
    /// The underlying stream
    pub(crate) inner: R,
    /// Number of bytes the next decode operation needs if known.
    decode_needs: Option<usize>,
    /// The buffer.
    buffer: Buffer,
    /// Whether there is any more items to return from the stream.  This is set to true once
    /// all decodable data in the buffer is returned and the underlying stream is closed.
    closed: bool,
}

impl<R: Read + Write + Unpin> ImapStream<R> {
    /// Creates a new `ImapStream` based on the given `Read`er.
    pub fn new(inner: R) -> Self {
        ImapStream {
            inner,
            buffer: Buffer::new(),
            decode_needs: None,
            closed: false,
        }
    }

    pub async fn encode(&mut self, msg: Request) -> Result<(), io::Error> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "inner stream closed",
            ));
        }
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

impl<R: Read + Write + Unpin> ImapStream<R> {
    fn maybe_decode(&mut self) -> io::Result<Option<ResponseData>> {
        if self.buffer.used() > self.decode_needs.unwrap_or(0) {
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
                    self.decode_needs = None;
                    self.buffer.reset_with_data(remaining);
                    Ok(response)
                }
                Err(nom::Err::Incomplete(Needed::Size(min))) => {
                    log::trace!("decode: incomplete data, need minimum {} bytes", min);
                    self.decode_needs = Some(usize::from(min));
                    Err(None)
                }
                Err(nom::Err::Incomplete(_)) => {
                    log::trace!("decode: incomplete data, need unknown number of bytes");
                    self.decode_needs = None;
                    Err(None)
                }
                Err(other) => {
                    self.decode_needs = None;
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

/// Abstraction around needed buffer management.
struct Buffer {
    /// The buffer itself.
    block: Block<'static>,
    /// Offset where used bytes range ends.
    offset: usize,
}

impl Buffer {
    const BLOCK_SIZE: usize = 1024 * 4;
    const MAX_CAPACITY: usize = 512 * 1024 * 1024; // 512 MiB

    fn new() -> Self {
        Self {
            block: POOL.alloc(Self::BLOCK_SIZE),
            offset: 0,
        }
    }

    /// Returns the number of bytes in the buffer containing data.
    fn used(&self) -> usize {
        self.offset
    }

    /// Returns the unused part of the buffer to which new data can be written.
    fn free_as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.block[self.offset..]
    }

    /// Indicate how many new bytes were written into the buffer.
    ///
    /// When new bytes are written into the slice returned by [tail_as_slice] this method
    /// should be called to extend the used portion of the buffer to include the new data.
    ///
    /// You can not write past the end of the buffer, so extending more then there is free
    /// space marks the entire buffer as used.
    // aka advance()?
    fn extend_used(&mut self, num_bytes: usize) {
        self.offset += num_bytes;
        if self.offset > self.block.size() {
            self.offset = self.block.size();
        }
    }

    /// Ensure the buffer has free capacity, optionally ensuring minimum buffer size.
    fn ensure_capacity(&mut self, required: Option<usize>) -> io::Result<()> {
        let free_bytes: usize = self.block.size() - self.offset;
        let min_required_bytes: usize = required.unwrap_or(0);
        let extra_bytes_needed: usize = min_required_bytes.saturating_sub(self.block.size());
        if free_bytes == 0 || extra_bytes_needed > 0 {
            let increase = std::cmp::max(Buffer::BLOCK_SIZE, extra_bytes_needed);
            self.grow(increase)?;
        }
        Ok(())
    }

    /// Grows the buffer, ensuring there are free bytes in the tail.
    ///
    /// The specified number of bytes is only a minimum.  The buffer could grow by more as
    /// it will always grow in multiples of [BLOCK_SIZE].
    ///
    /// If the size would be larger than [MAX_CAPACITY] an error is returned.
    // TODO: This bypasses the byte-pool block re-use.  That's bad.
    fn grow(&mut self, num_bytes: usize) -> io::Result<()> {
        let min_size = self.block.size() + num_bytes;
        let new_size = match min_size % Self::BLOCK_SIZE {
            0 => min_size,
            n => min_size + (Self::BLOCK_SIZE - n),
        };
        if new_size > Self::MAX_CAPACITY {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "incoming data too large",
            ))
        } else {
            self.block.realloc(new_size);
            Ok(())
        }
    }

    /// Return the block backing the buffer.
    ///
    /// Next you *must* either return this block using [return_block] or call
    /// [reset_with_data].
    // TODO: Enforce this with typestate.
    fn take_block(&mut self) -> Block<'static> {
        std::mem::replace(&mut self.block, POOL.alloc(Self::BLOCK_SIZE))
    }

    /// Reset the buffer to be a new allocation with given data copied in.
    ///
    /// This allows the previously returned block from [get_block] to be used in and owned
    /// by the [ResponseData].
    ///
    /// This does not do any bounds checking to see if the new buffer would exceed the
    /// maximum size.  It will however ensure that there is at least some free space at the
    /// end of the buffer so that the next reading operation won't need to realloc right
    /// away.  This could be wasteful if the next action on the buffer is another decode
    /// rather than a read, but we don't know.
    fn reset_with_data(&mut self, data: &[u8]) {
        let min_size = data.len();
        let new_size = match min_size % Self::BLOCK_SIZE {
            0 => min_size + Self::BLOCK_SIZE,
            n => min_size + (Self::BLOCK_SIZE - n),
        };
        self.block = POOL.alloc(new_size);
        self.block[..data.len()].copy_from_slice(data);
        self.offset = data.len();
    }

    /// Return the block which backs this buffer.
    fn return_block(&mut self, block: Block<'static>) {
        self.block = block;
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("used", &self.used())
            .field("capacity", &self.block.size())
            .finish()
    }
}

impl<R: Read + Write + Unpin> Stream for ImapStream<R> {
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    #[test]
    fn test_buffer_empty() {
        let buf = Buffer::new();
        assert_eq!(buf.used(), 0);

        let mut buf = Buffer::new();
        let slice: &[u8] = buf.free_as_mut_slice();
        assert_eq!(slice.len(), Buffer::BLOCK_SIZE);
        assert_eq!(slice.len(), buf.block.size());
    }

    #[test]
    fn test_buffer_extend_use() {
        let mut buf = Buffer::new();
        buf.extend_used(3);
        assert_eq!(buf.used(), 3);
        let slice = buf.free_as_mut_slice();
        assert_eq!(slice.len(), Buffer::BLOCK_SIZE - 3);

        // Extend past the end of the buffer.
        buf.extend_used(Buffer::BLOCK_SIZE);
        assert_eq!(buf.used(), Buffer::BLOCK_SIZE);
        assert_eq!(buf.offset, Buffer::BLOCK_SIZE);
        assert_eq!(buf.block.len(), buf.offset);
        let slice = buf.free_as_mut_slice();
        assert_eq!(slice.len(), 0);
    }

    #[test]
    fn test_buffer_write_read() {
        let mut buf = Buffer::new();
        let mut slice = buf.free_as_mut_slice();
        slice.write_all(b"hello").unwrap();
        buf.extend_used(b"hello".len());

        let slice = &buf.block[..buf.used()];
        assert_eq!(slice, b"hello");
        assert_eq!(buf.free_as_mut_slice().len(), buf.block.size() - buf.offset);
    }

    #[test]
    fn test_buffer_grow() {
        let mut buf = Buffer::new();
        assert_eq!(buf.block.size(), Buffer::BLOCK_SIZE);
        buf.grow(1).unwrap();
        assert_eq!(buf.block.size(), 2 * Buffer::BLOCK_SIZE);

        buf.grow(Buffer::BLOCK_SIZE + 1).unwrap();
        assert_eq!(buf.block.size(), 4 * Buffer::BLOCK_SIZE);

        let ret = buf.grow(Buffer::MAX_CAPACITY);
        assert!(ret.is_err());
    }

    #[test]
    fn test_buffer_ensure_capacity() {
        // Initial state: 1 byte capacity left, initial size.
        let mut buf = Buffer::new();
        buf.extend_used(Buffer::BLOCK_SIZE - 1);
        assert_eq!(buf.free_as_mut_slice().len(), 1);
        assert_eq!(buf.block.size(), Buffer::BLOCK_SIZE);

        // Still has capacity, no size request.
        buf.ensure_capacity(None).unwrap();
        assert_eq!(buf.free_as_mut_slice().len(), 1);
        assert_eq!(buf.block.size(), Buffer::BLOCK_SIZE);

        // No more capacity, initial size.
        buf.extend_used(1);
        assert_eq!(buf.free_as_mut_slice().len(), 0);
        assert_eq!(buf.block.size(), Buffer::BLOCK_SIZE);

        // No capacity, no size request.
        buf.ensure_capacity(None).unwrap();
        assert_eq!(buf.free_as_mut_slice().len(), Buffer::BLOCK_SIZE);
        assert_eq!(buf.block.size(), 2 * Buffer::BLOCK_SIZE);

        // Some capacity, size request.
        buf.extend_used(5);
        assert_eq!(buf.offset, Buffer::BLOCK_SIZE + 5);
        buf.ensure_capacity(Some(3 * Buffer::BLOCK_SIZE - 6))
            .unwrap();
        assert_eq!(buf.free_as_mut_slice().len(), 2 * Buffer::BLOCK_SIZE - 5);
        assert_eq!(buf.block.size(), 3 * Buffer::BLOCK_SIZE);
    }

    #[test]
    fn test_buffer_take_and_return_block() {
        // This test identifies blocks by their size.
        let mut buf = Buffer::new();
        buf.grow(1).unwrap();
        let block_size = buf.block.size();

        let block = buf.take_block();
        assert_eq!(block.size(), block_size);
        assert_ne!(buf.block.size(), block_size);

        buf.return_block(block);
        assert_eq!(buf.block.size(), block_size);
    }

    #[test]
    fn test_buffer_reset_with_data() {
        // This test identifies blocks by their size.
        let data: [u8; 2 * Buffer::BLOCK_SIZE] = [b'a'; 2 * Buffer::BLOCK_SIZE];
        let mut buf = Buffer::new();
        let block_size = buf.block.size();
        assert_eq!(block_size, Buffer::BLOCK_SIZE);
        buf.reset_with_data(&data);
        assert_ne!(buf.block.size(), block_size);
        assert_eq!(buf.block.size(), 3 * Buffer::BLOCK_SIZE);
        assert!(!buf.free_as_mut_slice().is_empty());

        let data: [u8; 0] = [];
        let mut buf = Buffer::new();
        buf.reset_with_data(&data);
        assert_eq!(buf.block.size(), Buffer::BLOCK_SIZE);
    }

    #[test]
    fn test_buffer_debug() {
        assert_eq!(
            format!("{:?}", Buffer::new()),
            format!(r#"Buffer {{ used: 0, capacity: {} }}"#, Buffer::BLOCK_SIZE)
        );
    }
}
