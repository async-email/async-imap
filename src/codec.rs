use std::ops::Deref;
use std::pin::Pin;

use async_std::io;
use async_std::stream::Stream;
use bytes::{BufMut, BytesMut};
use futures::task::{Context, Poll};
use futures_codec::{Decoder, Encoder};
use imap_proto::{RequestId, Response};
use nom::Needed;

#[derive(Debug)]
pub struct ImapCodec {
    decode_need_message_bytes: usize,
}

impl Default for ImapCodec {
    fn default() -> Self {
        Self {
            decode_need_message_bytes: 0,
        }
    }
}

impl<'a> Decoder for ImapCodec {
    type Item = ResponseData;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, io::Error> {
        if self.decode_need_message_bytes > buf.len() {
            return Ok(None);
        }

        // This is not efficient, but we accept this for now.
        // TODO: eventually improve on this
        let mut buf_vec = buf.to_vec();

        enum CreateError {
            None,
            Error(io::Error),
        }

        match ResponseData::try_new(buf_vec, |buf_vec| {
            let res = imap_proto::parse_response(&buf_vec);
            match res {
                Ok((remaining, response)) => {
                    let len = buf.len() - remaining.len();
                    // buf_vec.truncate(len);
                    Ok(response)
                }
                Err(nom::Err::Incomplete(Needed::Size(min))) => {
                    self.decode_need_message_bytes = min;
                    Err(CreateError::None)
                }
                Err(nom::Err::Incomplete(_)) => Err(CreateError::None),
                Err(err) => Err(CreateError::Error(io::Error::new(
                    io::ErrorKind::Other,
                    format!("{:?} during parsing of {:?}", err, buf),
                ))),
            }
        }) {
            Ok(response) => {
                self.decode_need_message_bytes = 0;
                Ok(Some(response))
            }
            Err(err) => match err {
                rental::RentalError(CreateError::None, _) => Ok(None),
                rental::RentalError(CreateError::Error(err), _) => Err(err),
            },
        }
    }
}

impl Encoder for ImapCodec {
    type Item = Request;
    type Error = io::Error;

    fn encode(&mut self, msg: Self::Item, dst: &mut BytesMut) -> Result<(), io::Error> {
        if let Some(tag) = msg.0 {
            // grow the buffer, as it does not grow automatically
            dst.reserve(tag.as_bytes().len() + 1);
            dst.put(tag.as_bytes());
            dst.put(b' ');
        }
        // grow the buffer, as it does not grow automatically
        dst.reserve(msg.1.len() + 2);
        dst.put(&msg.1);
        dst.put("\r\n");
        Ok(())
    }
}

rental! {
    pub mod rents {
        use super::*;

        #[rental(debug, covariant)]
        pub struct ResponseData {
            raw: Vec<u8>,
            response: Response<'raw>,
        }
    }
}

impl PartialEq for ResponseData {
    fn eq(&self, other: &Self) -> bool {
        self.head() == other.head()
    }
}

impl Eq for ResponseData {}

pub use rents::ResponseData;

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

    pub fn into_inner(self) -> Vec<u8> {
        self.into_head()
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct ConnStream<
    T: Stream<Item = io::Result<ResponseData>> + futures::Sink<Request, Error = io::Error> + Unpin,
> {
    pub(crate) stream: T,
}

impl<
        T: Stream<Item = io::Result<ResponseData>> + futures::Sink<Request, Error = io::Error> + Unpin,
    > ConnStream<T>
{
    unsafe_pinned!(stream: T);

    pub fn new(stream: T) -> Self {
        ConnStream { stream }
    }

    pub fn into_inner(self) -> T {
        self.stream
    }
}

impl<
        T: Stream<Item = io::Result<ResponseData>> + futures::Sink<Request, Error = io::Error> + Unpin,
    > futures::Sink<Request> for ConnStream<T>
{
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.stream().poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Request) -> Result<(), Self::Error> {
        self.stream().start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.stream().poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.stream().poll_close(cx)
    }
}

impl<
        T: Stream<Item = io::Result<ResponseData>> + futures::Sink<Request, Error = io::Error> + Unpin,
    > Stream for ConnStream<T>
{
    type Item = ResponseData;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let item = match futures::ready!(self.as_mut().stream().poll_next(cx)) {
            Some(e) => e,
            None => return Poll::Ready(None),
        };

        match item {
            Ok(res) => Poll::Ready(Some(res)),
            Err(err) => {
                eprintln!("Receive Error: {:#?}", err);
                Poll::Ready(None)
            }
        }
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

impl<
        T: Stream<Item = io::Result<ResponseData>> + futures::Sink<Request, Error = io::Error> + Unpin,
    > Unpin for ConnStream<T>
{
}
