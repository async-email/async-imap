//! Adds support for the IMAP IDLE command specificed in [RFC2177](https://tools.ietf.org/html/rfc2177).

use std::fmt;
use std::pin::Pin;
use std::time::Duration;

use async_std::io::{self, Read, Write};
use async_std::prelude::*;
use async_std::stream::Stream;
use futures::task::{Context, Poll};
use imap_proto::{RequestId, Response, Status};

use crate::client::Session;
use crate::codec::ResponseData;
use crate::error::Result;
use crate::parse::handle_unilateral;

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

/// A stream of server responses after sending `IDLE`. Created using [Handle::stream].
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct IdleStream<'a, St> {
    stream: &'a mut St,
}

impl<St: Unpin> Unpin for IdleStream<'_, St> {}

impl<'a, St: Stream + Unpin> IdleStream<'a, St> {
    unsafe_pinned!(stream: &'a mut St);

    pub(crate) fn new(stream: &'a mut St) -> Self {
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

/// Possible responses that happen on an open idle connection.
#[derive(Debug, PartialEq, Eq)]
pub enum IdleResponse {
    /// The manual interrupt was used to interrupt the idle connection..
    ManualInterrupt,
    /// The idle connection timed out, because of the user set timeout.
    Timeout,
    /// The server has indicated that some new action has happened.
    NewData(ResponseData),
}

impl<T: Read + Write + Unpin + fmt::Debug> Handle<T> {
    unsafe_pinned!(session: Session<T>);

    pub(crate) fn new(session: Session<T>) -> Handle<T> {
        Handle { session, id: None }
    }

    /// Start listening to the server side resonses.
    /// Must be called after [Handle::init].
    pub fn wait(
        &mut self,
    ) -> (
        impl Future<Output = IdleResponse> + '_,
        stop_token::StopSource,
    ) {
        assert!(
            self.id.is_some(),
            "Cannot listen to response without starting IDLE"
        );
        let sender = self.session.unsolicited_responses_tx.clone();

        let interrupt = stop_token::StopSource::new();
        let raw_stream = IdleStream::new(self);
        let mut interruptible_stream = interrupt.stop_token().stop_stream(raw_stream);

        let fut = async move {
            while let Some(resp) = interruptible_stream.next().await {
                match resp.parsed() {
                    Response::Data { status, .. } if status == &Status::Ok => {
                        // all good continue
                    }
                    Response::Continue { .. } => {
                        // continuation, wait for it
                    }
                    Response::Done { .. } => {
                        handle_unilateral(resp, sender.clone()).await;
                    }
                    _ => return IdleResponse::NewData(resp),
                }
            }

            IdleResponse::Timeout
        };

        (fut, interrupt)
    }

    /// Start listening to the server side resonses, stops latest after the passed in `timeout`.
    /// Must be called after [Handle::init].
    pub fn wait_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> (
        impl Future<Output = IdleResponse> + '_,
        stop_token::StopSource,
    ) {
        assert!(
            self.id.is_some(),
            "Cannot listen to response without starting IDLE"
        );

        let (waiter, interrupt) = self.wait();
        let fut = async move {
            match async_std::future::timeout(timeout, async move { waiter.await }).await {
                Ok(res) => res,
                Err(_err) => IdleResponse::Timeout,
            }
        };

        (fut, interrupt)
    }

    /// Initialise the idle connection by sending the `IDLE` command to the server.
    pub async fn init(&mut self) -> Result<()> {
        let id = self.session.run_command("IDLE").await?;
        self.id = Some(id);
        while let Some(res) = self.session.stream.next().await {
            match res.parsed() {
                Response::Continue { .. } => {
                    return Ok(());
                }
                Response::Done {
                    tag,
                    status,
                    information,
                    ..
                } => {
                    if tag == self.id.as_ref().unwrap() {
                        if let Status::Bad = status {
                            return Err(io::Error::new(
                                io::ErrorKind::ConnectionRefused,
                                information.unwrap().to_string(),
                            )
                            .into());
                        }
                    }
                    handle_unilateral(res, self.session.unsolicited_responses_tx.clone()).await;
                }
                _ => {
                    handle_unilateral(res, self.session.unsolicited_responses_tx.clone()).await;
                }
            }
        }

        Err(io::Error::new(io::ErrorKind::ConnectionRefused, "").into())
    }

    /// Signal that we want to exit the idle connection, by sending the `DONE`
    /// command to the server.
    pub async fn done(mut self) -> Result<Session<T>> {
        assert!(
            self.id.is_some(),
            "Cannot call DONE on a non initialized idle connection"
        );
        self.session.run_command_untagged("DONE").await?;
        let sender = self.session.unsolicited_responses_tx.clone();
        self.session
            .check_ok(self.id.expect("invalid setup"), Some(sender))
            .await?;

        Ok(self.session)
    }
}
