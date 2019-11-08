//! Adds support for the IMAP IDLE command specificed in [RFC2177](https://tools.ietf.org/html/rfc2177).

use async_std::io::{self, Read, Write};
use async_std::prelude::*;
use async_std::stream::Stream;
use futures::task::{Context, Poll};
use imap_proto::{RequestId, Response};
use std::fmt;
use std::pin::Pin;

use crate::client::Session;
use crate::codec::{Request, ResponseData};
use crate::error::Result;

/// `Handle` allows a client to block waiting for changes to the remote mailbox.
///
/// The handle blocks using the [`IDLE` command](https://tools.ietf.org/html/rfc2177#section-3)
/// specificed in [RFC 2177](https://tools.ietf.org/html/rfc2177) until the underlying server state
/// changes in some way. While idling does inform the client what changes happened on the server,
/// this implementation will currently just block until _anything_ changes, and then notify the
///
/// Note that the server MAY consider a client inactive if it has an IDLE command running, and if
/// such a server has an inactivity timeout it MAY log the client off implicitly at the end of its
/// timeout period.  Because of that, clients using IDLE are advised to terminate the IDLE and
/// re-issue it at least every 29 minutes to avoid being logged off. [`Handle::wait_keepalive`]
/// does this. This still allows a client to receive immediate mailbox updates even though it need
/// only "poll" at half hour intervals.
///
/// As long as a [`Handle`] is active, the mailbox cannot be otherwise accessed.
#[derive(Debug)]
pub struct Handle<T: Read + Write + Unpin + fmt::Debug> {
    session: Session<T>,
    id: Option<RequestId>,
}

impl<T: Read + Write + Unpin + fmt::Debug> Unpin for Handle<T> {}

impl<T: Read + Write + Unpin + fmt::Debug> Stream for Handle<T> {
    type Item = ResponseData;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.as_mut().session().get_stream().poll_next(cx)
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct IdleStream<'a, St> {
    stream: &'a mut St,
}

impl<St: Unpin> Unpin for IdleStream<'_, St> {}

impl<'a, St: Stream + Unpin> IdleStream<'a, St> {
    unsafe_pinned!(stream: &'a mut St);

    pub fn new(stream: &'a mut St) -> Self {
        IdleStream { stream }
    }
}

impl<St: futures::stream::FusedStream + Unpin> futures::stream::FusedStream for IdleStream<'_, St> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

impl<St: Stream + Unpin> Stream for IdleStream<'_, St> {
    type Item = St::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream().poll_next(cx)
    }
}

impl<T: Read + Write + Unpin + fmt::Debug> Handle<T> {
    unsafe_pinned!(session: Session<T>);

    pub fn new(session: Session<T>) -> Handle<T> {
        Handle { session, id: None }
    }

    pub fn stream(&mut self) -> IdleStream<'_, Self> {
        IdleStream::new(self)
    }

    pub async fn init(&mut self) -> Result<()> {
        let id = self.session.run_command("IDLE").await?;
        self.id = Some(id);
        while let Some(res) = self.session.stream.next().await {
            match res.parsed() {
                Response::Continue { .. } => {
                    return Ok(());
                }
                v => {
                    // TODO: send through unhandled responses
                    println!("unexpected response {:?}", v);
                }
            }
        }

        Err(io::Error::new(io::ErrorKind::ConnectionRefused, "").into())
    }

    pub async fn done(mut self) -> Result<Session<T>> {
        self.session.run_command_untagged("DONE").await?;
        self.session
            .check_ok(self.id.expect("invalid setup"))
            .await?;

        Ok(self.session)
    }
}
