use imap_proto::types::Capability as CapabilityRef;
use std::collections::hash_set::Iter;
use std::collections::HashSet;

const IMAP4REV1_CAPABILITY: &str = "IMAP4rev1";
const AUTH_CAPABILITY_PREFIX: &str = "AUTH=";

/// List of available Capabilities.
#[derive(Debug, Eq, PartialEq, Hash)]
pub enum Capability {
    /// The crucial imap capability.
    Imap4rev1,
    /// Auth type capability.
    Auth(String),
    /// Any other atoms.
    Atom(String),
}

impl From<&CapabilityRef<'_>> for Capability {
    fn from(c: &CapabilityRef<'_>) -> Self {
        match c {
            CapabilityRef::Imap4rev1 => Capability::Imap4rev1,
            CapabilityRef::Auth(s) => Capability::Auth(s.clone().into_owned()),
            CapabilityRef::Atom(s) => Capability::Atom(s.clone().into_owned()),
        }
    }
}

/// From [section 7.2.1 of RFC 3501](https://tools.ietf.org/html/rfc3501#section-7.2.1).
///
/// A list of capabilities that the server supports.
/// The capability list will include the atom "IMAP4rev1".
///
/// In addition, all servers implement the `STARTTLS`, `LOGINDISABLED`, and `AUTH=PLAIN` (described
/// in [IMAP-TLS](https://tools.ietf.org/html/rfc2595)) capabilities. See the [Security
/// Considerations section of the RFC](https://tools.ietf.org/html/rfc3501#section-11) for
/// important information.
///
/// A capability name which begins with `AUTH=` indicates that the server supports that particular
/// authentication mechanism.
///
/// The `LOGINDISABLED` capability indicates that the `LOGIN` command is disabled, and that the
/// server will respond with a [`super::Error::No`] response to any attempt to use the `LOGIN`
/// command even if the user name and password are valid.  An IMAP client MUST NOT issue the
/// `LOGIN` command if the server advertises the `LOGINDISABLED` capability.
///
/// Other capability names indicate that the server supports an extension, revision, or amendment
/// to the IMAP4rev1 protocol. Capability names either begin with `X` or they are standard or
/// standards-track [RFC 3501](https://tools.ietf.org/html/rfc3501) extensions, revisions, or
/// amendments registered with IANA.
///
/// Client implementations SHOULD NOT require any capability name other than `IMAP4rev1`, and MUST
/// ignore any unknown capability names.
pub struct Capabilities(pub(crate) HashSet<Capability>);

impl Capabilities {
    /// Check if the server has the given capability.
    pub fn has(&self, cap: &Capability) -> bool {
        self.0.contains(cap)
    }

    /// Check if the server has the given capability via str.
    pub fn has_str<S: AsRef<str>>(&self, cap: S) -> bool {
        let s = cap.as_ref();
        if s.eq_ignore_ascii_case(IMAP4REV1_CAPABILITY) {
            return self.has(&Capability::Imap4rev1);
        }
        if s.len() > AUTH_CAPABILITY_PREFIX.len() {
            let (pre, val) = s.split_at(AUTH_CAPABILITY_PREFIX.len());
            if pre.eq_ignore_ascii_case(AUTH_CAPABILITY_PREFIX) {
                return self.has(&Capability::Auth(val.into())); // TODO: avoid clone
            }
        }
        self.has(&Capability::Atom(s.into())) // TODO: avoid clone
    }

    /// Iterate over all the server's capabilities
    pub fn iter(&self) -> Iter<'_, Capability> {
        self.0.iter()
    }

    /// Returns how many capabilities the server has.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the server purports to have no capabilities.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
