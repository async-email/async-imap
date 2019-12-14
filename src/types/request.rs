use imap_proto::RequestId;

#[derive(Debug, Eq, PartialEq)]
pub struct Request(pub Option<RequestId>, pub Vec<u8>);
