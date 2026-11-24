use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCatalogError {
    InvalidCatalog(String),
    UnsupportedSchemaVersion(String),
    UnexpectedStatus(u16),
    Transport(String),
    Unavailable(String),
}

impl fmt::Display for ModelCatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCatalog(msg) => write!(f, "invalid catalog: {msg}"),
            Self::UnsupportedSchemaVersion(ver) => {
                write!(f, "unsupported catalog schema version: {ver}")
            }
            Self::UnexpectedStatus(code) => write!(f, "unexpected HTTP status: {code}"),
            Self::Transport(msg) => write!(f, "catalog transport error: {msg}"),
            Self::Unavailable(msg) => write!(f, "catalog unavailable: {msg}"),
        }
    }
}

impl std::error::Error for ModelCatalogError {}
