pub mod errors;
pub mod native;
pub mod registry;
pub mod sse;

pub use errors::NativeError;
pub use native::{NativeClient, NativeJsonResponse, NativeRequestConfig};
pub use registry::{
    validate_endpoint_url, AuthConfig, ModelOverride, NativeProtocol, ProviderEntry,
    ProviderRegistry, VerificationStatus,
};
pub use sse::{SseEvent, SseParser};
