use std;
use std::fmt::{self, Display};
use std::io;
use std::str::Utf8Error;

use serde::{de, ser};

pub type Result<T> = std::result::Result<T, Error>;

// This is a bare-bones implementation. A real library would provide additional
// information in its error type, for example the line and column at which the
// error occurred, the byte offset into the input, or the current key being
// processed.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Message(String),
    Utf8Error(Utf8Error),
    TrailingBytes,
    IntegerOverflow,
    LengthRequired,
    NonStringKey,
    MalformedTag,
}

impl ser::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl de::Error for Error {
    fn custom<T: Display>(msg: T) -> Self {
        Error::Message(msg.to_string())
    }
}

impl std::error::Error for Error {}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(err) => err.fmt(f),
            Error::Message(err) => err.fmt(f),
            Error::Utf8Error(err) => err.fmt(f),
            Error::TrailingBytes => "trailing bytes".fmt(f),
            Error::IntegerOverflow => "integer overflow".fmt(f),
            Error::LengthRequired => "length required".fmt(f),
            Error::NonStringKey => "non string key".fmt(f),
            Error::MalformedTag => "malformed tag".fmt(f),
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<Utf8Error> for Error {
    fn from(err: Utf8Error) -> Self {
        Error::Utf8Error(err)
    }
}
