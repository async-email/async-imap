use std::fmt;

use imap_proto::{RequestId, Response};

#[ouroboros::self_referencing(pub_extras)]
pub struct ResponseData {
    pub raw: Block<'static>,
    #[borrows(raw)]
    #[covariant]
    response: Response<'this>,
}

impl std::cmp::PartialEq for ResponseData {
    fn eq(&self, other: &Self) -> bool {
        self.parsed() == other.parsed()
    }
}

impl std::cmp::Eq for ResponseData {}

impl fmt::Debug for ResponseData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseData")
            .field("raw", &self.borrow_raw().len())
            .field("response", self.borrow_response())
            .finish()
    }
}

impl ResponseData {
    pub fn request_id(&self) -> Option<&RequestId> {
        match self.borrow_response() {
            Response::Done { ref tag, .. } => Some(tag),
            _ => None,
        }
    }

    pub fn parsed(&self) -> &Response<'_> {
        self.borrow_response()
    }
}
