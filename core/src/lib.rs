pub mod errors;
mod eventstream;
pub mod ffi;
pub mod lmp;
pub mod native;
pub mod registry;
pub mod sigv4;
pub mod sse;

pub use errors::NativeError;
pub use native::{NativeClient, NativeJsonResponse, NativeRequestConfig};
pub use registry::{
    validate_endpoint_url, AuthConfig, ModelOverride, NativeProtocol, ProviderEntry,
    ProviderRegistry, VerificationStatus,
};
pub use sse::{NdjsonParser, SseEvent, SseParser};
