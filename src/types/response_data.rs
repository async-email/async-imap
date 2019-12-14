use byte_pool::Block;
use imap_proto::{RequestId, Response};

rental! {
    pub mod rents {
        use super::*;

        #[rental(debug, covariant)]
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
