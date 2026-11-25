pub mod errors;
mod eventstream;
pub mod ffi;
mod ffi_normalizer;
mod ffi_stream;

pub use ffi_stream::LingxiStream;
pub mod lmp;
pub mod native;
pub mod registry;
pub mod sigv4;
pub mod sse;
pub mod stream;

pub use errors::NativeError;
pub use native::{NativeClient, NativeJsonResponse, NativeRequestConfig};
pub use stream::EventStreamHandle;
pub use registry::{
    validate_endpoint_url, AuthConfig, ModelOverride, NativeProtocol, ProviderEntry,
    ProviderRegistry, VerificationStatus,
};
pub use sse::{NdjsonParser, SseEvent, SseParser};
