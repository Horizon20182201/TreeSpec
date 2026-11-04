use thiserror::Error;

pub type Result<T> = std::result::Result<T, OrganizerError>;

#[derive(Debug, Error)]
pub enum OrganizerError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(transparent)]
    Database(#[from] rusqlite::Error),
}

pub(crate) fn fail(message: impl Into<String>) -> OrganizerError {
    OrganizerError::Message(message.into())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl From<rustix::io::Errno> for OrganizerError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TreeError {
    #[error("line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("unsupported TreeSpec version {0}; expected 1")]
    UnsupportedVersion(u32),
    #[error("invalid tree: {0}")]
    Validation(String),
    #[error("conflict for node {id:?}: {message}")]
    Conflict { id: String, message: String },
}

impl TreeError {
    pub(crate) fn at(line: usize, message: impl Into<String>) -> Self {
        Self::Parse {
            line,
            message: message.into(),
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}
