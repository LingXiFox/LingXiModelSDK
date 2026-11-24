use std::collections::HashMap;
use std::time::Duration;

use serde_json::{json, Value};

use crate::lmp::adapters::{normalize_response, normalize_stream_events, translate_request};
use crate::lmp::errors::LmpError;
use crate::lmp::types::{
    EmbeddingRequest, EmbeddingResponse, LmpRequest, LmpResponse, LmpStreamEvent, RerankRequest,
    RerankResponse, RerankResult, Usage,
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
            NativeProtocol::AwsBedrock => {
                self.native
                    .aws_bedrock_converse(&cfg, &req.model, &payload)
                    .await?
            }
            NativeProtocol::GoogleVertexAi => {
                self.native
                    .google_vertex_generate(&cfg, &req.model, &payload)
                    .await?
            }
            NativeProtocol::AzureOpenai => {
                self.native
                    .azure_openai_chat_completions(&cfg, &req.model, &payload)
                    .await?
            }
            NativeProtocol::Ollama => self.native.ollama_chat(&cfg, &payload).await?,
            NativeProtocol::Cohere => self.native.cohere_chat(&cfg, &payload).await?,
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
            NativeProtocol::AwsBedrock => {
                self.native
                    .stream_aws_bedrock_converse(&cfg, &req.model, &payload)
                    .await?
            }
            NativeProtocol::GoogleVertexAi => {
                self.native
                    .stream_google_vertex_generate(&cfg, &req.model, &payload)
                    .await?
            }
            NativeProtocol::AzureOpenai => {
                self.native
                    .stream_azure_openai_chat_completions(&cfg, &req.model, &payload)
                    .await?
            }
            NativeProtocol::Ollama => self.native.stream_ollama_chat(&cfg, &payload).await?,
            NativeProtocol::Cohere => self.native.stream_cohere_chat(&cfg, &payload).await?,
        };
        normalize_stream_events(protocol, &events)
    }

    pub async fn embed(
        &self,
        protocol: NativeProtocol,
        base_url: &str,
        api_key: Option<String>,
        req: &EmbeddingRequest,
    ) -> Result<EmbeddingResponse, LmpError> {
        if req.input.is_empty() {
            return Err(LmpError::InvalidRequest("embedding input is empty".into()));
        }
        let base = base_url.trim_end_matches('/');
        let mut headers = HashMap::new();
        let timeout = Duration::from_secs(60);

        match protocol {
            NativeProtocol::OpenaiChatCompletions => {
                let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
                    LmpError::InvalidRequest("bearer API key is required".into())
                })?;
                headers.insert("Authorization".into(), format!("Bearer {key}"));
                let url = format!("{base}/embeddings");
                let payload = build_openai_embedding_payload(req);
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, timeout, false)
                    .await?;
                normalize_embedding_response(protocol, &resp.body)
            }
            NativeProtocol::AzureOpenai => {
                let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
                    LmpError::InvalidRequest("Azure api-key is required".into())
                })?;
                headers.insert("api-key".into(), key);
                let url = format!(
                    "{base}/openai/deployments/{}/embeddings?api-version=2024-10-21",
                    req.model
                );
                let payload = json!({"input": req.input});
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, timeout, false)
                    .await?;
                normalize_embedding_response(protocol, &resp.body)
            }
            NativeProtocol::GoogleGemini => {
                let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
                    LmpError::InvalidRequest("x-goog-api-key is required".into())
                })?;
                headers.insert("x-goog-api-key".into(), key);
                let url = format!("{base}/v1beta/models/{}:embedContent", req.model);
                let payload = json!({"content": {"parts": [{"text": req.input[0]}]}});
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, timeout, false)
                    .await?;
                normalize_embedding_response(protocol, &resp.body)
            }
            NativeProtocol::GoogleVertexAi => {
                let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
                    LmpError::InvalidRequest("Vertex bearer token is required".into())
                })?;
                headers.insert("Authorization".into(), format!("Bearer {key}"));
                let url = format!(
                    "{base}/v1/publishers/google/models/{}:embedContent",
                    req.model
                );
                let payload = json!({"content": {"parts": [{"text": req.input[0]}]}});
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, timeout, false)
                    .await?;
                normalize_embedding_response(protocol, &resp.body)
            }
            NativeProtocol::Ollama => {
                let url = format!("{base}/api/embed");
                let payload = json!({"model": req.model, "input": req.input});
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, timeout, false)
                    .await?;
                normalize_embedding_response(protocol, &resp.body)
            }
            NativeProtocol::Cohere => {
                let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
                    LmpError::InvalidRequest("Cohere bearer token is required".into())
                })?;
                headers.insert("Authorization".into(), format!("Bearer {key}"));
                let url = format!("{base}/v2/embed");
                let payload = json!({
                    "model": req.model,
                    "texts": req.input,
                    "input_type": "search_document"
                });
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, timeout, false)
                    .await?;
                normalize_embedding_response(protocol, &resp.body)
            }
            NativeProtocol::OpenaiResponses
            | NativeProtocol::AnthropicMessages
            | NativeProtocol::AwsBedrock => Err(LmpError::UnsupportedFeature {
                protocol: protocol.as_str().to_string(),
                feature: "embed".into(),
            }),
        }
    }

    pub async fn rerank(
        &self,
        protocol: NativeProtocol,
        base_url: &str,
        api_key: Option<String>,
        req: &RerankRequest,
    ) -> Result<RerankResponse, LmpError> {
        match protocol {
            NativeProtocol::Cohere => {
                let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
                    LmpError::InvalidRequest("Cohere bearer token is required".into())
                })?;
                let mut headers = HashMap::new();
                headers.insert("Authorization".into(), format!("Bearer {key}"));
                let url = format!("{}/v2/rerank", base_url.trim_end_matches('/'));
                let mut payload = json!({
                    "model": req.model,
                    "query": req.query,
                    "documents": req.documents
                });
                if let Some(top_n) = req.top_n {
                    payload["top_n"] = json!(top_n);
                }
                let resp = self
                    .native
                    .post_json_endpoint(&url, &headers, &payload, Duration::from_secs(60), false)
                    .await?;
                let results = resp.body["results"]
                    .as_array()
                    .ok_or_else(|| LmpError::Decode("missing results array".into()))?
                    .iter()
                    .map(|r| RerankResult {
                        index: r["index"].as_u64().unwrap_or(0) as usize,
                        relevance_score: r["relevance_score"].as_f64().unwrap_or(0.0),
                    })
                    .collect();
                Ok(RerankResponse {
                    results,
                    raw: resp.body,
                })
            }
            _ => Err(LmpError::UnsupportedFeature {
                protocol: protocol.as_str().to_string(),
                feature: "rerank".into(),
            }),
        }
    }
}

pub fn normalize_embedding_response(
    protocol: NativeProtocol,
    raw: &Value,
) -> Result<EmbeddingResponse, LmpError> {
    match protocol {
        NativeProtocol::OpenaiChatCompletions | NativeProtocol::AzureOpenai => {
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
        NativeProtocol::GoogleGemini | NativeProtocol::GoogleVertexAi => {
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
        NativeProtocol::Ollama => {
            let arr = raw["embeddings"]
                .as_array()
                .ok_or_else(|| LmpError::Decode("missing embeddings array".into()))?;
            let embeddings = arr
                .iter()
                .map(|row| row.as_array().into_iter().flatten().filter_map(Value::as_f64).collect())
                .collect();
            Ok(EmbeddingResponse {
                embeddings,
                usage: Usage {
                    input_tokens: raw["prompt_eval_count"].as_u64().unwrap_or(0),
                    total_tokens: raw["prompt_eval_count"].as_u64().unwrap_or(0),
                    ..Default::default()
                },
                raw: raw.clone(),
            })
        }
        NativeProtocol::Cohere => {
            let arr = raw["embeddings"]["float"]
                .as_array()
                .ok_or_else(|| LmpError::Decode("missing embeddings.float array".into()))?;
            let embeddings = arr
                .iter()
                .map(|row| row.as_array().into_iter().flatten().filter_map(Value::as_f64).collect())
                .collect();
            Ok(EmbeddingResponse {
                embeddings,
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
