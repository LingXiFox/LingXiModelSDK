use std::fmt;

use crate::errors::NativeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LmpError {
    UnsupportedFeature { protocol: String, feature: String },
    Auth(String),
    RateLimit(String),
    InvalidRequest(String),
    ProviderError { status: u16, body: String },
    Timeout(String),
    Cancelled(String),
    Transport(String),
    Decode(String),
}

impl fmt::Display for LmpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFeature { protocol, feature } => {
                write!(f, "unsupported feature '{feature}' for protocol '{protocol}'")
            }
            Self::Auth(msg) => write!(f, "authentication error: {msg}"),
            Self::RateLimit(msg) => write!(f, "rate limit exceeded: {msg}"),
            Self::InvalidRequest(msg) => write!(f, "invalid request: {msg}"),
            Self::ProviderError { status, body } => write!(f, "provider HTTP {status}: {body}"),
            Self::Timeout(msg) => write!(f, "request timed out: {msg}"),
            Self::Cancelled(msg) => write!(f, "request cancelled: {msg}"),
            Self::Transport(msg) => write!(f, "transport error: {msg}"),
            Self::Decode(msg) => write!(f, "decode error: {msg}"),
        }
    }
}

impl LmpError {
    /// Stable machine-readable code carried by terminal stream `error` events.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedFeature { .. } => "unsupported_feature",
            Self::Auth(_) => "auth",
            Self::RateLimit(_) => "rate_limit",
            Self::InvalidRequest(_) => "invalid_request",
            Self::ProviderError { .. } => "provider_error",
            Self::Timeout(_) => "timeout",
            Self::Cancelled(_) => "cancelled",
            Self::Transport(_) => "transport",
            Self::Decode(_) => "decode",
        }
    }
}

impl std::error::Error for LmpError {}

impl From<NativeError> for LmpError {
    fn from(err: NativeError) -> Self {
        match err {
            NativeError::AuthMissing(msg) => Self::Auth(msg),
            NativeError::UnsupportedProtocol { protocol, .. } => Self::UnsupportedFeature {
                protocol,
                feature: "protocol".into(),
            },
            NativeError::HttpStatus { status, body, .. } => {
                let text = String::from_utf8_lossy(&body).to_string();
                match status {
                    401 | 403 => Self::Auth(text),
                    429 => Self::RateLimit(text),
                    400 | 422 => Self::InvalidRequest(text),
                    _ => Self::ProviderError { status, body: text },
                }
            }
            NativeError::Transport(msg) => {
                if msg.to_lowercase().contains("timed out") {
                    Self::Timeout(msg)
                } else {
                    Self::Transport(msg)
                }
            }
            NativeError::Timeout(msg) => Self::Timeout(msg),
            NativeError::Cancelled(msg) => Self::Cancelled(msg),
            NativeError::StreamParse(msg) => Self::Decode(msg),
            NativeError::Registry(msg)
            | NativeError::InvalidEndpoint(msg)
            | NativeError::InvalidRequest(msg) => Self::InvalidRequest(msg),
        }
    }
}
