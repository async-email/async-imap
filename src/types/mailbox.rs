use super::{Flag, Uid};
use std::fmt;

/// Meta-information about an IMAP mailbox, as returned by
/// [`SELECT`](https://tools.ietf.org/html/rfc3501#section-6.3.1) and friends.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct Mailbox {
    /// Defined flags in the mailbox.  See the description of the [FLAGS
    /// response](https://tools.ietf.org/html/rfc3501#section-7.2.6) for more detail.
    pub flags: Vec<Flag<'static>>,

    /// The number of messages in the mailbox.  See the description of the [EXISTS
    /// response](https://tools.ietf.org/html/rfc3501#section-7.3.1) for more detail.
    pub exists: u32,

    /// The number of messages with the \Recent flag set. See the description of the [RECENT
    /// response](https://tools.ietf.org/html/rfc3501#section-7.3.2) for more detail.
    pub recent: u32,

    /// The message sequence number of the first unseen message in the mailbox.  If this is
    /// missing, the client can not make any assumptions about the first unseen message in the
    /// mailbox, and needs to issue a `SEARCH` command if it wants to find it.
    pub unseen: Option<u32>,

    /// A list of message flags that the client can change permanently.  If this is missing, the
    /// client should assume that all flags can be changed permanently. If the client attempts to
    /// STORE a flag that is not in this list list, the server will either ignore the change or
    /// store the state change for the remainder of the current session only.
    pub permanent_flags: Vec<Flag<'static>>,

    /// The next unique identifier value.  If this is missing, the client can not make any
    /// assumptions about the next unique identifier value.
    pub uid_next: Option<Uid>,

    /// The unique identifier validity value.  See [`Uid`] for more details.  If this is missing,
    /// the server does not support unique identifiers.
    pub uid_validity: Option<u32>,
}

impl fmt::Display for Mailbox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "flags: {:?}, exists: {}, recent: {}, unseen: {:?}, permanent_flags: {:?},\
             uid_next: {:?}, uid_validity: {:?}",
            self.flags,
            self.exists,
            self.recent,
            self.unseen,
            self.permanent_flags,
            self.uid_next,
            self.uid_validity
        )
    }
}
