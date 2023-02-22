//! IMAP error types.

use std::io::Error as IoError;
use std::str::Utf8Error;

use base64::DecodeError;

/// A convenience wrapper around `Result` for `imap::Error`.
pub type Result<T> = std::result::Result<T, Error>;

/// A set of errors that can occur in the IMAP client
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// An `io::Error` that occurred while trying to read or write to a network stream.
    #[error("io: {0}")]
    Io(#[from] IoError),
    /// A BAD response from the IMAP server.
    #[error("bad response: {0}")]
    Bad(String),
    /// A NO response from the IMAP server.
    #[error("no response: {0}")]
    No(String),
    /// The connection was terminated unexpectedly.
    #[error("connection lost")]
    ConnectionLost,
    /// Error parsing a server response.
    #[error("parse: {0}")]
    Parse(#[from] ParseError),
    /// Command inputs were not valid [IMAP
    /// strings](https://tools.ietf.org/html/rfc3501#section-4.3).
    #[error("validate: {0}")]
    Validate(#[from] ValidateError),
    /// Error appending an e-mail.
    #[error("could not append mail to mailbox")]
    Append,
}

/// An error occured while trying to parse a server response.
#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    /// Indicates an error parsing the status response. Such as OK, NO, and BAD.
    #[error("unable to parse status response")]
    Invalid(Vec<u8>),
    /// An unexpected response was encountered.
    #[error("encountered unexpected parsed response: {0}")]
    Unexpected(String),
    /// The client could not find or decode the server's authentication challenge.
    #[error("unable to parse authentication response: {0} - {1:?}")]
    Authentication(String, Option<DecodeError>),
    /// The client received data that was not UTF-8 encoded.
    #[error("unable to parse data ({0:?}) as UTF-8 text: {1:?}")]
    DataNotUtf8(Vec<u8>, #[source] Utf8Error),
    /// The expected response for X was not found
    #[error("expected response not found for: {0}")]
    ExpectedResponseNotFound(String),
}

/// An [invalid character](https://tools.ietf.org/html/rfc3501#section-4.3) was found in an input
/// string.
#[derive(thiserror::Error, Debug)]
#[error("invalid character in input: '{0}'")]
pub struct ValidateError(pub char);

#[cfg(test)]
mod tests {
    use super::*;

    fn is_send<T: Send>(_t: T) {}

    #[test]
    fn test_send() {
        is_send::<Result<usize>>(Ok(3));
    }
}
