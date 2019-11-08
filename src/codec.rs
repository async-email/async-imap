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

        let (response, rsp_len) = match imap_proto::parse_response(buf) {
            Ok((remaining, response)) => {
                // This SHOULD be acceptable/safe: BytesMut storage memory is
                // allocated on the heap and should not move. It will not be
                // freed as long as we keep a reference alive, which we do
                // by retaining a reference to the split buffer, below.
                let response = unsafe { std::mem::transmute(response) };
                // println!("got response: {:?}", &response);
                (response, buf.len() - remaining.len())
            }
            Err(nom::Err::Incomplete(Needed::Size(min))) => {
                self.decode_need_message_bytes = min;
                return Ok(None);
            }
            Err(nom::Err::Incomplete(_)) => return Ok(None),
            Err(err) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("{:?} during parsing of {:?}", err, buf),
                ))
            }
        };

        let raw = buf.split_to(rsp_len).freeze().to_vec();
        self.decode_need_message_bytes = 0;
        Ok(Some(ResponseData { raw, response }))
    }
}

impl Encoder for ImapCodec {
    type Item = Request;
    type Error = io::Error;
    fn encode(&mut self, msg: Self::Item, dst: &mut BytesMut) -> Result<(), io::Error> {
        println!("writing {:#?}", std::str::from_utf8(&msg.1));

        if let Some(tag) = msg.0 {
            dst.put(tag.as_bytes());
            dst.put(b' ');
        }
        dst.put(&msg.1);
        dst.put("\r\n");
        Ok(())
    }
}

// TODO: use rental for this
#[derive(Debug, PartialEq, Eq)]
pub struct ResponseData {
    pub raw: Vec<u8>,
    // This reference is really scoped to the lifetime of the `raw`
    // member, but unfortunately Rust does not allow that yet. It
    // is transmuted to `'static` by the `Decoder`, instead, and
    // references returned to callers of `ResponseData` are limited
    // to the lifetime of the `ResponseData` struct.
    //
    // `raw` is never mutated during the lifetime of `ResponseData`,
    // and `Response` does not not implement any specific drop glue.
    pub response: Response<'static>,
}

impl Deref for ResponseData {
    type Target = Response<'static>;

    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl ResponseData {
    pub fn request_id(&self) -> Option<&RequestId> {
        match self.response {
            Response::Done { ref tag, .. } => Some(tag),
            _ => None,
        }
    }
    pub fn parsed(&self) -> &Response<'_> {
        &self.response
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.raw
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
                println!("error: {:#?}", err);
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
