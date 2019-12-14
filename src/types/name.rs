use crate::codec::ResponseData;
use std::borrow::Cow;

use imap_proto::{MailboxDatum, Response};

rental! {
    pub mod rents {
        use super::*;

        /// A name that matches a `LIST` or `LSUB` command.
        #[rental(debug, covariant)]
        pub struct Name {
            data: Vec<u8>,
            inner: InnerName<'data>,
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub struct InnerName<'a> {
    attributes: Vec<NameAttribute<'a>>,
    delimiter: Option<&'a str>,
    name: &'a str,
}

pub use rents::Name;

/// An attribute set for an IMAP name.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum NameAttribute<'a> {
    /// It is not possible for any child levels of hierarchy to exist
    /// under this name; no child levels exist now and none can be
    /// created in the future.
    NoInferiors,

    /// It is not possible to use this name as a selectable mailbox.
    NoSelect,

    /// The mailbox has been marked "interesting" by the server; the
    /// mailbox probably contains messages that have been added since
    /// the last time the mailbox was selected.
    Marked,

    /// The mailbox does not contain any additional messages since the
    /// last time the mailbox was selected.
    Unmarked,

    /// A non-standard user- or server-defined name attribute.
    Custom(Cow<'a, str>),
}

impl NameAttribute<'static> {
    fn system(s: &str) -> Option<Self> {
        match s {
            "\\Noinferiors" => Some(NameAttribute::NoInferiors),
            "\\Noselect" => Some(NameAttribute::NoSelect),
            "\\Marked" => Some(NameAttribute::Marked),
            "\\Unmarked" => Some(NameAttribute::Unmarked),
            _ => None,
        }
    }
}

impl<'a> From<String> for NameAttribute<'a> {
    fn from(s: String) -> Self {
        if let Some(f) = NameAttribute::system(&s) {
            f
        } else {
            NameAttribute::Custom(Cow::Owned(s))
        }
    }
}

impl<'a> From<&'a str> for NameAttribute<'a> {
    fn from(s: &'a str) -> Self {
        if let Some(f) = NameAttribute::system(s) {
            f
        } else {
            NameAttribute::Custom(Cow::Borrowed(s))
        }
    }
}

impl Name {
    pub(crate) fn from_mailbox_data(resp: ResponseData) -> Self {
        unimplemented!()
        // let ResponseData { raw, response } = resp;

        // // TODO: no to_vec
        // match response {
        //     Response::MailboxData(MailboxDatum::List {
        //         flags,
        //         delimiter,
        //         name,
        //     }) => Name::new(raw.to_vec(), |_data| InnerName {
        //         attributes: flags.iter().map(|s| NameAttribute::from(*s)).collect(),
        //         delimiter,
        //         name,
        //     }),
        //     _ => panic!("cannot construct from non mailbox data"),
        // }
    }

    /// Attributes of this name.
    pub fn attributes(&self) -> &[NameAttribute<'_>] {
        &self.suffix().attributes[..]
    }

    /// The hierarchy delimiter is a character used to delimit levels of hierarchy in a mailbox
    /// name.  A client can use it to create child mailboxes, and to search higher or lower levels
    /// of naming hierarchy.  All children of a top-level hierarchy node use the same
    /// separator character.  `None` means that no hierarchy exists; the name is a "flat" name.
    pub fn delimiter(&self) -> Option<&str> {
        self.suffix().delimiter
    }

    /// The name represents an unambiguous left-to-right hierarchy, and are valid for use as a
    /// reference in `LIST` and `LSUB` commands. Unless [`NameAttribute::NoSelect`] is indicated,
    /// the name is also valid as an argument for commands, such as `SELECT`, that accept mailbox
    /// names.
    pub fn name(&self) -> &str {
        self.suffix().name
    }
}
