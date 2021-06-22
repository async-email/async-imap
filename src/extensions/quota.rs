//! Adds support for the GETQUOTA and GETQUOTAROOT commands specificed in [RFC2087](https://tools.ietf.org/html/rfc2087).

use async_std::channel;
use async_std::io;
use async_std::prelude::*;
use async_std::stream::Stream;
use imap_proto::{self, Quota, QuotaRoot, RequestId, Response};

use crate::types::*;
use crate::{
    error::Result,
    parse::{filter_sync, handle_unilateral},
};
use crate::{
    error::{Error, ParseError},
    types::ResponseData,
};

pub(crate) async fn parse_get_quota<T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &mut T,
    unsolicited: channel::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<Quota<'_>> {
    let mut quota = None;
    while let Some(resp) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let resp = resp?;
        match resp.parsed() {
            Response::Quota(q) => quota = Some(q.clone().into_owned()),
            _ => {
                handle_unilateral(resp, unsolicited.clone()).await;
            }
        }
    }

    match quota {
        Some(q) => Ok(q),
        None => Err(Error::Parse(ParseError::ExpectedResponseNotFound(
            "Quota, no quota response found".to_string(),
        ))),
    }
}

pub(crate) async fn parse_get_quota_root<T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &mut T,
    unsolicited: channel::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<(Vec<QuotaRoot<'_>>, Vec<Quota<'_>>)> {
    let mut roots: Vec<QuotaRoot<'_>> = Vec::new();
    let mut quotas: Vec<Quota<'_>> = Vec::new();

    while let Some(resp) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let resp = resp?;
        match resp.parsed() {
            Response::QuotaRoot(qr) => {
                roots.push(qr.clone().into_owned());
            }
            Response::Quota(q) => {
                quotas.push(q.clone().into_owned());
            }
            _ => {
                handle_unilateral(resp, unsolicited.clone()).await;
            }
        }
    }

    Ok((roots, quotas))
}
