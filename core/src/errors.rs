use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeError {
    Registry(String),
    /// The caller's request is missing something the protocol requires.
    InvalidRequest(String),
    InvalidEndpoint(String),
    AuthMissing(String),
    UnsupportedProtocol { provider: String, protocol: String },
    HttpStatus {
        status: u16,
        body: Vec<u8>,
        headers: HashMap<String, String>,
    },
    Transport(String),
    Timeout(String),
    Cancelled(String),
    StreamParse(String),
}

impl fmt::Display for NativeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Registry(msg) => write!(f, "registry error: {msg}"),
            Self::InvalidRequest(msg) => write!(f, "invalid request: {msg}"),
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
            Self::Timeout(msg) => write!(f, "stream timed out: {msg}"),
            Self::Cancelled(msg) => write!(f, "stream cancelled: {msg}"),
            Self::StreamParse(msg) => write!(f, "SSE parse error: {msg}"),
        }
    }
}

impl NativeError {
    /// Stable machine-readable code for C ABI and cross-language error handling.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Registry(_) => "registry",
            Self::InvalidRequest(_) => "invalid_request",
            Self::InvalidEndpoint(_) => "invalid_endpoint",
            Self::AuthMissing(_) => "auth_missing",
            Self::UnsupportedProtocol { .. } => "unsupported_protocol",
            Self::HttpStatus { .. } => "http_status",
            Self::Transport(_) => "transport",
            Self::Timeout(_) => "timeout",
            Self::Cancelled(_) => "cancelled",
            Self::StreamParse(_) => "stream_parse",
        }
    }
}

impl std::error::Error for NativeError {}
