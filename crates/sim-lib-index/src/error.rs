//! Error type for runtime index exploration.

use std::fmt;

/// Runtime index command error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexError {
    message: String,
}

impl IndexError {
    /// Builds an error from displayable text.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for IndexError {}

impl From<String> for IndexError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for IndexError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}
