//! Adds support for the IMAP METADATA command specificed in [RFC5464](https://tools.ietf.org/html/rfc5464).

use std::fmt::Debug;

use async_std::io::{self, Read, Write};
use async_std::prelude::*;
use async_std::stream::Stream;
use async_std::sync;
use imap_proto::types::{MailboxDatum, Metadata};
use imap_proto::{RequestId, Response};

use crate::client::{validate_str, Session};
use crate::error::Result;
use crate::parse::{filter_sync, handle_unilateral};
use crate::types::ResponseData;
use crate::types::UnsolicitedResponse;

fn format_as_cmd_list_item(metadata: &Metadata) -> String {
    format!(
        "{} {}",
        validate_str(metadata.entry.as_str()).unwrap(),
        metadata
            .value
            .as_ref()
            .map(|v| validate_str(v.as_str()).unwrap())
            .unwrap_or_else(|| "NIL".to_string())
    )
}

/// Represents variants of DEPTH parameter for GETMETADATA command
/// See RFC5464 section 4.2.2 "DEPTH GETMETADATA Command Option"
/// https://tools.ietf.org/html/rfc5464#section-4.2.2
#[derive(Debug, Copy, Clone)]
pub enum MetadataDepth {
    /// Depth 0 for get metadata, no entries below the specified one are returned
    Zero,
    /// Depth 1 for get metadata, only entries immediately below the specified one are returned
    One,
    /// Depth infinity for get metadata, all entries below the specified one are returned
    Inf,
}

impl MetadataDepth {
    fn depth_str<'a>(self) -> &'a str {
        match self {
            MetadataDepth::Zero => "0",
            MetadataDepth::One => "1",
            MetadataDepth::Inf => "infinity",
        }
    }
}

async fn parse_metadata<'a, T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &'a mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<Vec<Metadata>> {
    let mut res: Vec<Metadata> = Vec::new();
    while let Some(Ok(resp)) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let unsolicited_h = unsolicited.clone();
        let parsed_resp = resp.parsed();
        match parsed_resp {
            Response::MailboxData(MailboxDatum::MetadataSolicited { values, .. }) => {
                res.append(&mut values.to_vec());
            }
            Response::MailboxData(MailboxDatum::MetadataUnsolicited { mailbox, values }) => {
                unsolicited
                    .send(UnsolicitedResponse::Metadata {
                        mailbox: (*mailbox).to_string(),
                        metadata_entries: values.iter().map(|s| (*s).to_string()).collect(),
                    })
                    .await;
            }
            _ => handle_unilateral(resp, unsolicited_h).await,
        };
    }
    Ok(res)
}

/// Sends GETMETADATA command to the server and returns the list of entries and their values.
pub(crate) async fn get_metadata_impl<'a, S: AsRef<str>, T: Read + Write + Debug + Unpin>(
    session: &'a mut Session<T>,
    mbox: S,
    entries: &[S],
    depth: MetadataDepth,
    maxsize: Option<usize>,
) -> Result<Vec<Metadata>> {
    let v: Vec<String> = entries
        .iter()
        .map(|e| validate_str(e.as_ref()).unwrap())
        .collect();
    let s = v.as_slice().join(" ");
    let mut command = format!("GETMETADATA (DEPTH {}", depth.depth_str());

    if let Some(size) = maxsize {
        command.push_str(format!(" MAXSIZE {}", size).as_str());
    }

    command.push_str(format!(") {} ({})", validate_str(mbox.as_ref()).unwrap(), s).as_str());
    let id = session.run_command(command).await?;
    let unsolicited = session.unsolicited_responses_tx.clone();
    let pinned_session = std::pin::Pin::new(session);
    let mut stream = pinned_session.get_stream().get_mut();
    let mdata = parse_metadata(&mut stream, unsolicited, id).await?;
    Ok(mdata)
}

/// Sends SETMETADATA command to the server and checks if it was executed successfully.
pub(crate) async fn set_metadata_impl<'a, S: AsRef<str>, T: Read + Write + Debug + Unpin>(
    session: &'a mut Session<T>,
    mbox: S,
    keyval: &[Metadata],
) -> Result<()> {
    let v: Vec<String> = keyval
        .iter()
        .map(|metadata| format_as_cmd_list_item(metadata))
        .collect();
    let s = v.as_slice().join(" ");
    let command = format!("SETMETADATA {} ({})", validate_str(mbox.as_ref())?, s);
    session.run_command_and_check_ok(command).await?;
    Ok(())
}
