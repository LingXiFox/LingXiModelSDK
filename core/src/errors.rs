use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeError {
    Registry(String),
    InvalidEndpoint(String),
    AuthMissing(String),
    UnsupportedProtocol { provider: String, protocol: String },
    HttpStatus {
        status: u16,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    },
    Transport(String),
    StreamParse(String),
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(msg) => write!(f, "registry error: {msg}"),
            Self::InvalidEndpoint(msg) => write!(f, "invalid endpoint: {msg}"),
            Self::AuthMissing(msg) => write!(f, "missing authentication: {msg}"),
            Self::UnsupportedProtocol { provider, protocol } => {
                write!(f, "provider '{provider}' does not declare protocol '{protocol}'")
            }
            Self::HttpStatus { status, body, .. } => {
                let text = String::from_utf8_lossy(body);
                write!(f, "upstream HTTP {status}: {text}")
            }
            Self::Transport(msg) => write!(f, "transport error: {msg}"),
            Self::StreamParse(msg) => write!(f, "SSE parse error: {msg}"),
        }
    }
}

impl std::error::Error for NativeError {}
