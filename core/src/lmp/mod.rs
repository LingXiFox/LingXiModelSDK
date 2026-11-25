pub mod adapters;
pub mod client;
pub mod errors;
pub mod types;

pub use adapters::{normalize_response, normalize_stream_events, translate_request};
pub use client::{
    build_openai_embedding_payload, normalize_embedding_response, LmpStreamHandle, UnifiedClient,
};
pub use errors::LmpError;
pub use types::{
    ContentBlock, EmbeddingRequest, EmbeddingResponse, LmpMessage, LmpRequest, LmpResponse,
    LmpStreamEvent, RerankRequest, RerankResponse, RerankResult, ResponseFormat, Role,
    ThinkingConfig, ToolChoice, ToolDefinition, Usage,
};
