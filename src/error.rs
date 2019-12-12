//! IMAP error types.

use std::error::Error as StdError;
use std::fmt;
use std::io::Error as IoError;
use std::result;
use std::str::Utf8Error;

use base64::DecodeError;
use imap_proto::Response;

/// A convenience wrapper around `Result` for `imap::Error`.
pub type Result<T> = result::Result<T, Error>;

/// A set of errors that can occur in the IMAP client
#[derive(Debug)]
pub enum Error {
    /// An `io::Error` that occurred while trying to read or write to a network stream.
    Io(IoError),
    /// A BAD response from the IMAP server.
    Bad(String),
    /// A NO response from the IMAP server.
    No(String),
    /// The connection was terminated unexpectedly.
    ConnectionLost,
    /// Error parsing a server response.
    Parse(ParseError),
    /// Command inputs were not valid [IMAP
    /// strings](https://tools.ietf.org/html/rfc3501#section-4.3).
    Validate(ValidateError),
    /// Error appending an e-mail.
    Append,
    #[doc(hidden)]
    __Nonexhaustive,
}

impl From<IoError> for Error {
    fn from(err: IoError) -> Error {
        Error::Io(err)
    }
}

impl From<ParseError> for Error {
    fn from(err: ParseError) -> Error {
        Error::Parse(err)
    }
}

impl<'a> From<&'a Response<'a>> for Error {
    fn from(err: &'a Response<'a>) -> Error {
        Error::Parse(ParseError::Unexpected(format!("{:?}", err)))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Error::Io(ref e) => fmt::Display::fmt(e, f),
            Error::Validate(ref e) => fmt::Display::fmt(e, f),
            Error::No(ref data) | Error::Bad(ref data) => {
                write!(f, "{}: {}", &String::from(self.description()), data)
            }
            ref e => f.write_str(e.description()),
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Io(ref e) => e.description(),
            Error::Parse(ref e) => e.description(),
            Error::Validate(ref e) => e.description(),
            Error::Bad(_) => "Bad Response",
            Error::No(_) => "No Response",
            Error::ConnectionLost => "Connection lost",
            Error::Append => "Could not append mail to mailbox",
            Error::__Nonexhaustive => "Unknown",
        }
    }

    fn cause(&self) -> Option<&dyn StdError> {
        match *self {
            Error::Io(ref e) => Some(e),
            Error::Parse(ParseError::DataNotUtf8(_, ref e)) => Some(e),
            _ => None,
        }
    }
}

/// An error occured while trying to parse a server response.
#[derive(Debug)]
pub enum ParseError {
    /// Indicates an error parsing the status response. Such as OK, NO, and BAD.
    Invalid(Vec<u8>),
    /// An unexpected response was encountered.
    Unexpected(String),
    /// The client could not find or decode the server's authentication challenge.
    Authentication(String, Option<DecodeError>),
    /// The client received data that was not UTF-8 encoded.
    DataNotUtf8(Vec<u8>, Utf8Error),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ref e => f.write_str(e.description()),
        }
    }
}

impl StdError for ParseError {
    fn description(&self) -> &str {
        match *self {
            ParseError::Invalid(_) => "Unable to parse status response",
            ParseError::Unexpected(_) => "Encountered unexpected parsed response",
            ParseError::Authentication(_, _) => "Unable to parse authentication response",
            ParseError::DataNotUtf8(_, _) => "Unable to parse data as UTF-8 text",
        }
    }

    fn cause(&self) -> Option<&dyn StdError> {
        match *self {
            ParseError::Authentication(_, Some(ref e)) => Some(e),
            _ => None,
        }
    }
}

/// An [invalid character](https://tools.ietf.org/html/rfc3501#section-4.3) was found in an input
/// string.
#[derive(Debug)]
pub struct ValidateError(pub char);

impl fmt::Display for ValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // print character in debug form because invalid ones are often whitespaces
        write!(f, "{}: {:?}", self.description(), self.0)
    }
}

impl StdError for ValidateError {
    fn description(&self) -> &str {
        "Invalid character in input"
    }

    fn cause(&self) -> Option<&dyn StdError> {
        None
    }
}
