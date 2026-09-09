//! Recoverable owner failures. Never include a MAC in the display text.

use std::fmt;
use std::io;

/// ConnectRPC client or serve failure.
#[derive(Debug)]
pub enum Error {
    /// Filesystem or loopback I/O.
    Io(io::Error),
    /// Human line (no MAC).
    Message(String),
}

impl Error {
    /// Build a [`Error::Message`].
    #[must_use]
    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<connectrpc::ConnectError> for Error {
    fn from(error: connectrpc::ConnectError) -> Self {
        Self::Message(error.to_string())
    }
}

impl From<remote_debug_host::Error> for Error {
    fn from(error: remote_debug_host::Error) -> Self {
        Self::Message(error.to_string())
    }
}
