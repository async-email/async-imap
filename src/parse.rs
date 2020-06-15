use std::collections::HashSet;

use async_std::io;
use async_std::prelude::*;
use async_std::stream::Stream;
use async_std::sync;
use imap_proto::{self, MailboxDatum, RequestId, Response};

use crate::error::{Error, Result};
use crate::types::ResponseData;
use crate::types::*;

pub(crate) fn parse_names<'a, T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &'a mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> impl Stream<Item = Result<Name>> + 'a {
    use futures::StreamExt;

    StreamExt::filter_map(
        StreamExt::take_while(stream, move |res| filter(res, &command_tag)),
        move |resp| {
            let unsolicited = unsolicited.clone();
            async move {
                match resp {
                    Ok(resp) => match resp.parsed() {
                        Response::MailboxData(MailboxDatum::List { .. }) => {
                            let name = Name::from_mailbox_data(resp);
                            Some(Ok(name))
                        }
                        _ => {
                            handle_unilateral(resp, unsolicited).await;
                            None
                        }
                    },
                    Err(err) => Some(Err(err.into())),
                }
            }
        },
    )
}

fn filter(res: &io::Result<ResponseData>, command_tag: &RequestId) -> impl Future<Output = bool> {
    let val = filter_sync(res, command_tag);
    futures::future::ready(val)
}

pub(crate) fn filter_sync(res: &io::Result<ResponseData>, command_tag: &RequestId) -> bool {
    match res {
        Ok(res) => match res.parsed() {
            Response::Done { tag, .. } => tag != command_tag,
            _ => true,
        },
        Err(_err) => false,
    }
}

pub(crate) fn parse_fetches<'a, T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &'a mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> impl Stream<Item = Result<Fetch>> + 'a {
    use futures::StreamExt;

    StreamExt::filter_map(
        StreamExt::take_while(stream, move |res| filter(res, &command_tag)),
        move |resp| {
            let unsolicited = unsolicited.clone();

            async move {
                match resp {
                    Ok(resp) => match resp.parsed() {
                        Response::Fetch(..) => Some(Ok(Fetch::new(resp))),
                        _ => {
                            handle_unilateral(resp, unsolicited).await;
                            None
                        }
                    },
                    Err(err) => Some(Err(err.into())),
                }
            }
        },
    )
}

pub(crate) fn parse_expunge<'a, T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &'a mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> impl Stream<Item = Result<u32>> + 'a {
    use futures::StreamExt;

    StreamExt::filter_map(
        StreamExt::take_while(stream, move |res| filter(res, &command_tag)),
        move |resp| {
            let unsolicited = unsolicited.clone();

            async move {
                match resp {
                    Ok(resp) => match resp.parsed() {
                        Response::Expunge(id) => Some(Ok(*id)),
                        _ => {
                            handle_unilateral(resp, unsolicited).await;
                            None
                        }
                    },
                    Err(err) => Some(Err(err.into())),
                }
            }
        },
    )
}

pub(crate) async fn parse_capabilities<'a, T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &'a mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<Capabilities> {
    let mut caps: HashSet<Capability> = HashSet::new();

    while let Some(resp) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let resp = resp?;
        match resp.parsed() {
            Response::Capabilities(cs) => {
                for c in cs {
                    caps.insert(Capability::from(c)); // TODO: avoid clone
                }
            }
            _ => {
                handle_unilateral(resp, unsolicited.clone()).await;
            }
        }
    }

    Ok(Capabilities(caps))
}

pub(crate) async fn parse_noop<T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<()> {
    while let Some(resp) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let resp = resp?;
        handle_unilateral(resp, unsolicited.clone()).await;
    }

    Ok(())
}

pub(crate) async fn parse_mailbox<T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<Mailbox> {
    let mut mailbox = Mailbox::default();

    while let Some(resp) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let resp = resp?;
        match resp.parsed() {
            Response::Data {
                status,
                code,
                information,
            } => {
                use imap_proto::Status;

                match status {
                    Status::Ok => {
                        use imap_proto::ResponseCode;
                        match code {
                            Some(ResponseCode::UidValidity(uid)) => {
                                mailbox.uid_validity = Some(*uid);
                            }
                            Some(ResponseCode::UidNext(unext)) => {
                                mailbox.uid_next = Some(*unext);
                            }
                            Some(ResponseCode::Unseen(n)) => {
                                mailbox.unseen = Some(*n);
                            }
                            Some(ResponseCode::PermanentFlags(flags)) => {
                                mailbox
                                    .permanent_flags
                                    .extend(flags.iter().map(|s| (*s).to_string()).map(Flag::from));
                            }
                            _ => {}
                        }
                    }
                    Status::Bad => {
                        return Err(Error::Bad(format!(
                            "code: {:?}, info: {:?}",
                            code, information
                        )))
                    }
                    Status::No => {
                        return Err(Error::No(format!(
                            "code: {:?}, info: {:?}",
                            code, information
                        )))
                    }
                    _ => {
                        return Err(Error::Io(io::Error::new(
                            io::ErrorKind::Other,
                            format!(
                                "status: {:?}, code: {:?}, information: {:?}",
                                status, code, information
                            ),
                        )));
                    }
                }
            }
            Response::MailboxData(m) => match m {
                MailboxDatum::Status { .. } => handle_unilateral(resp, unsolicited.clone()).await,
                MailboxDatum::Exists(e) => {
                    mailbox.exists = *e;
                }
                MailboxDatum::Recent(r) => {
                    mailbox.recent = *r;
                }
                MailboxDatum::Flags(flags) => {
                    mailbox
                        .flags
                        .extend(flags.iter().map(|s| (*s).to_string()).map(Flag::from));
                }
                MailboxDatum::List { .. } => {}
                MailboxDatum::MetadataSolicited { .. } => {}
                MailboxDatum::MetadataUnsolicited { .. } => {}
            },
            _ => {
                handle_unilateral(resp, unsolicited.clone()).await;
            }
        }
    }

    Ok(mailbox)
}

pub(crate) async fn parse_ids<T: Stream<Item = io::Result<ResponseData>> + Unpin>(
    stream: &mut T,
    unsolicited: sync::Sender<UnsolicitedResponse>,
    command_tag: RequestId,
) -> Result<HashSet<u32>> {
    let mut ids: HashSet<u32> = HashSet::new();

    while let Some(resp) = stream
        .take_while(|res| filter_sync(res, &command_tag))
        .next()
        .await
    {
        let resp = resp?;
        match resp.parsed() {
            Response::IDs(cs) => {
                for c in cs {
                    ids.insert(*c);
                }
            }
            _ => {
                handle_unilateral(resp, unsolicited.clone()).await;
            }
        }
    }

    Ok(ids)
}

// check if this is simply a unilateral server response
// (see Section 7 of RFC 3501):
pub(crate) async fn handle_unilateral(
    res: ResponseData,
    unsolicited: sync::Sender<UnsolicitedResponse>,
) {
    match res.parsed() {
        Response::MailboxData(MailboxDatum::Status { mailbox, status }) => {
            unsolicited
                .send(UnsolicitedResponse::Status {
                    mailbox: (*mailbox).into(),
                    attributes: status
                        .iter()
                        .map(|s| match s {
                            // Fake clone
                            StatusAttribute::HighestModSeq(a) => StatusAttribute::HighestModSeq(*a),
                            StatusAttribute::Messages(a) => StatusAttribute::Messages(*a),
                            StatusAttribute::Recent(a) => StatusAttribute::Recent(*a),
                            StatusAttribute::UidNext(a) => StatusAttribute::UidNext(*a),
                            StatusAttribute::UidValidity(a) => StatusAttribute::UidValidity(*a),
                            StatusAttribute::Unseen(a) => StatusAttribute::Unseen(*a),
                        })
                        .collect(),
                })
                .await;
        }
        Response::MailboxData(MailboxDatum::Recent(n)) => {
            unsolicited.send(UnsolicitedResponse::Recent(*n)).await;
        }
        Response::MailboxData(MailboxDatum::Exists(n)) => {
            unsolicited.send(UnsolicitedResponse::Exists(*n)).await;
        }
        Response::Expunge(n) => {
            unsolicited.send(UnsolicitedResponse::Expunge(*n)).await;
        }
        _ => {
            unsolicited.send(UnsolicitedResponse::Other(res)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_stream(data: &[&str]) -> Vec<io::Result<ResponseData>> {
        data.iter()
            .map(|line| {
                let mut block = crate::imap_stream::POOL.alloc(line.as_bytes().len());
                block.copy_from_slice(line.as_bytes());
                ResponseData::try_new(block, |bytes| -> io::Result<_> {
                    let (remaining, response) = imap_proto::Response::from_bytes(bytes).unwrap();
                    assert_eq!(remaining.len(), 0);
                    Ok(response)
                })
                .map_err(|err: rental::RentalError<io::Error, _>| err.0)
            })
            .collect()
    }

    #[async_std::test]
    async fn parse_capability_test() {
        let expected_capabilities = vec!["IMAP4rev1", "STARTTLS", "AUTH=GSSAPI", "LOGINDISABLED"];
        let responses = input_stream(&vec![
            "* CAPABILITY IMAP4rev1 STARTTLS AUTH=GSSAPI LOGINDISABLED\r\n",
        ]);

        let mut stream = async_std::stream::from_iter(responses);
        let (send, recv) = sync::channel(10);
        let id = RequestId("A0001".into());
        let capabilities = parse_capabilities(&mut stream, send, id).await.unwrap();
        // shouldn't be any unexpected responses parsed
        assert!(recv.is_empty());
        assert_eq!(capabilities.len(), 4);
        for e in expected_capabilities {
            assert!(capabilities.has_str(e));
        }
    }

    #[async_std::test]
    async fn parse_capability_case_insensitive_test() {
        // Test that "IMAP4REV1" (instead of "IMAP4rev1") is accepted
        let expected_capabilities = vec!["IMAP4rev1", "STARTTLS"];
        let responses = input_stream(&vec!["* CAPABILITY IMAP4REV1 STARTTLS\r\n"]);
        let mut stream = async_std::stream::from_iter(responses);

        let (send, recv) = sync::channel(10);
        let id = RequestId("A0001".into());
        let capabilities = parse_capabilities(&mut stream, send, id).await.unwrap();

        // shouldn't be any unexpected responses parsed
        assert!(recv.is_empty());
        assert_eq!(capabilities.len(), 2);
        for e in expected_capabilities {
            assert!(capabilities.has_str(e));
        }
    }

    #[async_std::test]
    #[should_panic]
    async fn parse_capability_invalid_test() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![
            "* JUNK IMAP4rev1 STARTTLS AUTH=GSSAPI LOGINDISABLED\r\n",
        ]);
        let mut stream = async_std::stream::from_iter(responses);

        let id = RequestId("A0001".into());
        parse_capabilities(&mut stream, send.clone(), id)
            .await
            .unwrap();
        assert!(recv.is_empty());
    }

    #[async_std::test]
    async fn parse_names_test() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec!["* LIST (\\HasNoChildren) \".\" \"INBOX\"\r\n"]);
        let mut stream = async_std::stream::from_iter(responses);

        let id = RequestId("A0001".into());
        let names: Vec<_> = parse_names(&mut stream, send, id)
            .collect::<Result<Vec<Name>>>()
            .await
            .unwrap();
        assert!(recv.is_empty());
        assert_eq!(names.len(), 1);
        assert_eq!(
            names[0].attributes(),
            &[NameAttribute::from("\\HasNoChildren")]
        );
        assert_eq!(names[0].delimiter(), Some("."));
        assert_eq!(names[0].name(), "INBOX");
    }

    #[async_std::test]
    async fn parse_fetches_empty() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![]);
        let mut stream = async_std::stream::from_iter(responses);
        let id = RequestId("a".into());

        let fetches = parse_fetches(&mut stream, send, id)
            .collect::<Result<Vec<_>>>()
            .await
            .unwrap();
        assert!(recv.is_empty());
        assert!(fetches.is_empty());
    }

    #[async_std::test]
    async fn parse_fetches_test() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![
            "* 24 FETCH (FLAGS (\\Seen) UID 4827943)\r\n",
            "* 25 FETCH (FLAGS (\\Seen))\r\n",
        ]);
        let mut stream = async_std::stream::from_iter(responses);
        let id = RequestId("a".into());

        let fetches = parse_fetches(&mut stream, send, id)
            .collect::<Result<Vec<_>>>()
            .await
            .unwrap();
        assert!(recv.is_empty());

        assert_eq!(fetches.len(), 2);
        assert_eq!(fetches[0].message, 24);
        assert_eq!(fetches[0].flags().collect::<Vec<_>>(), vec![Flag::Seen]);
        assert_eq!(fetches[0].uid, Some(4827943));
        assert_eq!(fetches[0].body(), None);
        assert_eq!(fetches[0].header(), None);
        assert_eq!(fetches[1].message, 25);
        assert_eq!(fetches[1].flags().collect::<Vec<_>>(), vec![Flag::Seen]);
        assert_eq!(fetches[1].uid, None);
        assert_eq!(fetches[1].body(), None);
        assert_eq!(fetches[1].header(), None);
    }

    #[async_std::test]
    async fn parse_fetches_w_unilateral() {
        // https://github.com/mattnenterprise/rust-imap/issues/81
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec!["* 37 FETCH (UID 74)\r\n", "* 1 RECENT\r\n"]);
        let mut stream = async_std::stream::from_iter(responses);
        let id = RequestId("a".into());

        let fetches = parse_fetches(&mut stream, send, id)
            .collect::<Result<Vec<_>>>()
            .await
            .unwrap();
        assert_eq!(recv.recv().await, Some(UnsolicitedResponse::Recent(1)));

        assert_eq!(fetches.len(), 1);
        assert_eq!(fetches[0].message, 37);
        assert_eq!(fetches[0].uid, Some(74));
    }

    #[async_std::test]
    async fn parse_names_w_unilateral() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![
            "* LIST (\\HasNoChildren) \".\" \"INBOX\"\r\n",
            "* 4 EXPUNGE\r\n",
        ]);
        let mut stream = async_std::stream::from_iter(responses);

        let id = RequestId("A0001".into());
        let names = parse_names(&mut stream, send, id)
            .collect::<Result<Vec<_>>>()
            .await
            .unwrap();

        assert_eq!(recv.recv().await, Some(UnsolicitedResponse::Expunge(4)));

        assert_eq!(names.len(), 1);
        assert_eq!(
            names[0].attributes(),
            &[NameAttribute::from("\\HasNoChildren")]
        );
        assert_eq!(names[0].delimiter(), Some("."));
        assert_eq!(names[0].name(), "INBOX");
    }

    #[async_std::test]
    async fn parse_capabilities_w_unilateral() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![
            "* CAPABILITY IMAP4rev1 STARTTLS AUTH=GSSAPI LOGINDISABLED\r\n",
            "* STATUS dev.github (MESSAGES 10 UIDNEXT 11 UIDVALIDITY 1408806928 UNSEEN 0)\r\n",
            "* 4 EXISTS\r\n",
        ]);
        let mut stream = async_std::stream::from_iter(responses);

        let expected_capabilities = vec!["IMAP4rev1", "STARTTLS", "AUTH=GSSAPI", "LOGINDISABLED"];

        let id = RequestId("A0001".into());
        let capabilities = parse_capabilities(&mut stream, send, id).await.unwrap();

        assert_eq!(capabilities.len(), 4);
        for e in expected_capabilities {
            assert!(capabilities.has_str(e));
        }

        assert_eq!(
            recv.recv().await.unwrap(),
            UnsolicitedResponse::Status {
                mailbox: "dev.github".to_string(),
                attributes: vec![
                    StatusAttribute::Messages(10),
                    StatusAttribute::UidNext(11),
                    StatusAttribute::UidValidity(1408806928),
                    StatusAttribute::Unseen(0)
                ]
            }
        );
        assert_eq!(recv.recv().await.unwrap(), UnsolicitedResponse::Exists(4));
    }

    #[async_std::test]
    async fn parse_ids_w_unilateral() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![
            "* SEARCH 23 42 4711\r\n",
            "* 1 RECENT\r\n",
            "* STATUS INBOX (MESSAGES 10 UIDNEXT 11 UIDVALIDITY 1408806928 UNSEEN 0)\r\n",
        ]);
        let mut stream = async_std::stream::from_iter(responses);

        let id = RequestId("A0001".into());
        let ids = parse_ids(&mut stream, send, id).await.unwrap();

        assert_eq!(ids, [23, 42, 4711].iter().cloned().collect());

        assert_eq!(recv.recv().await.unwrap(), UnsolicitedResponse::Recent(1));
        assert_eq!(
            recv.recv().await.unwrap(),
            UnsolicitedResponse::Status {
                mailbox: "INBOX".to_string(),
                attributes: vec![
                    StatusAttribute::Messages(10),
                    StatusAttribute::UidNext(11),
                    StatusAttribute::UidValidity(1408806928),
                    StatusAttribute::Unseen(0)
                ]
            }
        );
    }

    #[async_std::test]
    async fn parse_ids_test() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec![
                "* SEARCH 1600 1698 1739 1781 1795 1885 1891 1892 1893 1898 1899 1901 1911 1926 1932 1933 1993 1994 2007 2032 2033 2041 2053 2062 2063 2065 2066 2072 2078 2079 2082 2084 2095 2100 2101 2102 2103 2104 2107 2116 2120 2135 2138 2154 2163 2168 2172 2189 2193 2198 2199 2205 2212 2213 2221 2227 2267 2275 2276 2295 2300 2328 2330 2332 2333 2334\r\n",
                "* SEARCH 2335 2336 2337 2338 2339 2341 2342 2347 2349 2350 2358 2359 2362 2369 2371 2372 2373 2374 2375 2376 2377 2378 2379 2380 2381 2382 2383 2384 2385 2386 2390 2392 2397 2400 2401 2403 2405 2409 2411 2414 2417 2419 2420 2424 2426 2428 2439 2454 2456 2467 2468 2469 2490 2515 2519 2520 2521\r\n",
            ]);
        let mut stream = async_std::stream::from_iter(responses);

        let id = RequestId("A0001".into());
        let ids = parse_ids(&mut stream, send, id).await.unwrap();

        assert!(recv.is_empty());
        let ids: HashSet<u32> = ids.iter().cloned().collect();
        assert_eq!(
            ids,
            [
                1600, 1698, 1739, 1781, 1795, 1885, 1891, 1892, 1893, 1898, 1899, 1901, 1911, 1926,
                1932, 1933, 1993, 1994, 2007, 2032, 2033, 2041, 2053, 2062, 2063, 2065, 2066, 2072,
                2078, 2079, 2082, 2084, 2095, 2100, 2101, 2102, 2103, 2104, 2107, 2116, 2120, 2135,
                2138, 2154, 2163, 2168, 2172, 2189, 2193, 2198, 2199, 2205, 2212, 2213, 2221, 2227,
                2267, 2275, 2276, 2295, 2300, 2328, 2330, 2332, 2333, 2334, 2335, 2336, 2337, 2338,
                2339, 2341, 2342, 2347, 2349, 2350, 2358, 2359, 2362, 2369, 2371, 2372, 2373, 2374,
                2375, 2376, 2377, 2378, 2379, 2380, 2381, 2382, 2383, 2384, 2385, 2386, 2390, 2392,
                2397, 2400, 2401, 2403, 2405, 2409, 2411, 2414, 2417, 2419, 2420, 2424, 2426, 2428,
                2439, 2454, 2456, 2467, 2468, 2469, 2490, 2515, 2519, 2520, 2521
            ]
            .iter()
            .cloned()
            .collect()
        );
    }

    #[async_std::test]
    async fn parse_ids_search() {
        let (send, recv) = sync::channel(10);
        let responses = input_stream(&vec!["* SEARCH\r\n"]);
        let mut stream = async_std::stream::from_iter(responses);

        let id = RequestId("A0001".into());
        let ids = parse_ids(&mut stream, send, id).await.unwrap();

        assert!(recv.is_empty());
        let ids: HashSet<u32> = ids.iter().cloned().collect();
        assert_eq!(ids, HashSet::<u32>::new());
    }
}
