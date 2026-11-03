use thiserror::Error;

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
