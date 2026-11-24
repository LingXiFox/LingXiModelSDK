use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};

use crate::lmp::adapters::{normalize_response, normalize_stream_events, translate_request};
use crate::lmp::errors::LmpError;
use crate::lmp::types::{
    EmbeddingRequest, EmbeddingResponse, LmpRequest, LmpResponse, LmpStreamEvent, RerankRequest,
    RerankResponse, Usage,
};
use crate::native::{NativeClient, NativeRequestConfig};
use crate::registry::NativeProtocol;

pub struct UnifiedClient {
    native: NativeClient,
}

impl Default for UnifiedClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UnifiedClient {
    pub fn new() -> Self {
        Self {
            native: NativeClient::new(),
        }
    }

    pub async fn complete(
        &self,
        protocol: NativeProtocol,
        base_url: &str,
        api_key: Option<String>,
        req: &LmpRequest,
    ) -> Result<LmpResponse, LmpError> {
        let payload = translate_request(protocol, req)?;
        let cfg = NativeRequestConfig {
            base_url: base_url.to_string(),
            api_key,
            headers: HashMap::new(),
            timeout: Duration::from_millis(req.timeout_ms.unwrap_or(60_000)),
            allow_insecure_http: false,
        };
        let resp = match protocol {
            NativeProtocol::OpenaiChatCompletions => {
                self.native.openai_chat_completions(&cfg, &payload).await?
            }
            NativeProtocol::OpenaiResponses => self.native.openai_responses(&cfg, &payload).await?,
            NativeProtocol::AnthropicMessages => {
                self.native.anthropic_messages(&cfg, &payload).await?
            }
            NativeProtocol::GoogleGemini => {
                self.native
                    .google_gemini_generate(&cfg, &req.model, &payload)
                    .await?
            }
        };
        normalize_response(protocol, &resp.body)
    }

    pub async fn stream(
        &self,
        protocol: NativeProtocol,
        base_url: &str,
        api_key: Option<String>,
        req: &LmpRequest,
    ) -> Result<Vec<LmpStreamEvent>, LmpError> {
        let payload = translate_request(protocol, req)?;
        let cfg = NativeRequestConfig {
            base_url: base_url.to_string(),
            api_key,
            headers: HashMap::new(),
            timeout: Duration::from_millis(req.timeout_ms.unwrap_or(60_000)),
            allow_insecure_http: false,
        };
        let events = match protocol {
            NativeProtocol::OpenaiChatCompletions => {
                self.native
                    .stream_openai_chat_completions(&cfg, &payload)
                    .await?
            }
            NativeProtocol::OpenaiResponses => {
                self.native.stream_openai_responses(&cfg, &payload).await?
            }
            NativeProtocol::AnthropicMessages => {
                self.native
                    .stream_anthropic_messages(&cfg, &payload)
                    .await?
            }
            NativeProtocol::GoogleGemini => {
                self.native
                    .stream_google_gemini_generate(&cfg, &req.model, &payload)
                    .await?
            }
        };
        normalize_stream_events(protocol, &events)
    }

    pub async fn embed(
        &self,
        protocol: NativeProtocol,
        _base_url: &str,
        _api_key: Option<String>,
        req: &EmbeddingRequest,
    ) -> Result<EmbeddingResponse, LmpError> {
        match protocol {
            NativeProtocol::OpenaiChatCompletions | NativeProtocol::GoogleGemini => {
                if req.input.is_empty() {
                    return Err(LmpError::InvalidRequest("embedding input is empty".into()));
                }
                Err(LmpError::InvalidRequest(
                    "embed requires live or mock HTTP endpoint; use embed_with_response or NativeClient"
                        .into(),
                ))
            }
            NativeProtocol::OpenaiResponses | NativeProtocol::AnthropicMessages => {
                Err(LmpError::UnsupportedFeature {
                    protocol: protocol.as_str().to_string(),
                    feature: "embed".into(),
                })
            }
        }
    }

    pub async fn rerank(
        &self,
        protocol: NativeProtocol,
        _req: &RerankRequest,
    ) -> Result<RerankResponse, LmpError> {
        Err(LmpError::UnsupportedFeature {
            protocol: protocol.as_str().to_string(),
            feature: "rerank".into(),
        })
    }
}

pub fn normalize_embedding_response(
    protocol: NativeProtocol,
    raw: &Value,
) -> Result<EmbeddingResponse, LmpError> {
    match protocol {
        NativeProtocol::OpenaiChatCompletions => {
            let data = raw["data"]
                .as_array()
                .ok_or_else(|| LmpError::Decode("missing data array".into()))?;
            let mut embeddings = Vec::with_capacity(data.len());
            for item in data {
                let vec: Vec<f64> = item["embedding"]
                    .as_array()
                    .ok_or_else(|| LmpError::Decode("missing embedding array".into()))?
                    .iter()
                    .filter_map(Value::as_f64)
                    .collect();
                embeddings.push(vec);
            }
            Ok(EmbeddingResponse {
                embeddings,
                usage: Usage {
                    input_tokens: raw["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                    output_tokens: 0,
                    total_tokens: raw["usage"]["total_tokens"].as_u64().unwrap_or(0),
                    ..Default::default()
                },
                raw: raw.clone(),
            })
        }
        NativeProtocol::GoogleGemini => {
            let values: Vec<f64> = raw["embedding"]["values"]
                .as_array()
                .ok_or_else(|| LmpError::Decode("missing embedding.values".into()))?
                .iter()
                .filter_map(Value::as_f64)
                .collect();
            Ok(EmbeddingResponse {
                embeddings: vec![values],
                usage: Usage::default(),
                raw: raw.clone(),
            })
        }
        _ => Err(LmpError::UnsupportedFeature {
            protocol: protocol.as_str().to_string(),
            feature: "embed".into(),
        }),
    }
}

pub fn build_openai_embedding_payload(req: &EmbeddingRequest) -> Value {
    json!({"model": req.model, "input": req.input})
}
