use std::fmt;

use byte_pool::Block;
use imap_proto::{RequestId, Response};

rental! {
    pub mod rents {
        use super::*;

        #[rental(covariant)]
        pub struct ResponseData {
            raw: Block<'static>,
            response: Response<'raw>,
        }
    }
}

pub use rents::ResponseData;

impl std::cmp::PartialEq for ResponseData {
    fn eq(&self, other: &Self) -> bool {
        self.parsed() == other.parsed()
    }
}

impl std::cmp::Eq for ResponseData {}

impl fmt::Debug for ResponseData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseData")
            .field("raw", &self.head().len())
            .field("response", self.suffix())
            .finish()
    }
}

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
}
