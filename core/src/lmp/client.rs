use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use serde_json::{json, Value};

use crate::lmp::adapters::{
    normalize_response, normalize_stream_events, translate_request, StreamNormalizer,
};
use crate::lmp::errors::LmpError;
use crate::lmp::types::{
    EmbeddingRequest, EmbeddingResponse, LmpRequest, LmpResponse, LmpStreamEvent, RerankRequest,
    RerankResponse, RerankResult, Usage,
};
use crate::native::{NativeClient, NativeRequestConfig};
use crate::registry::NativeProtocol;
use crate::stream::EventStreamHandle;

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
            stream_idle_timeout: Duration::from_secs(120),
            allow_insecure_http: false,
        };
        let resp = self.native.request_json(protocol, &cfg, Some(&req.model), &payload).await?;
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
            stream_idle_timeout: Duration::from_secs(120),
            allow_insecure_http: false,
        };
        let events = self.native.request_stream(protocol, &cfg, Some(&req.model), &payload).await?;
        normalize_stream_events(protocol, &events)
    }
    /// Opens a unified (LMP) stream without buffering.
    ///
    /// Events are normalized incrementally; the caller sees each fragment as
    /// soon as it is decoded. Terminal failures surface as a final
    /// `LmpStreamEvent::Error { code, message }` so C ABIs and language SDKs can
    /// distinguish a clean end from a truncated stream.
    pub async fn open_stream(
        &self,
        protocol: NativeProtocol,
        base_url: &str,
        api_key: Option<String>,
        req: &LmpRequest,
    ) -> Result<LmpStreamHandle, LmpError> {
        let payload = translate_request(protocol, req)?;
        let cfg = NativeRequestConfig {
            base_url: base_url.to_string(),
            api_key,
            headers: HashMap::new(),
            timeout: Duration::from_millis(req.timeout_ms.unwrap_or(60_000)),
            stream_idle_timeout: Duration::from_secs(120),
            allow_insecure_http: false,
        };
        let inner = self
            .native
            .open_stream_events(protocol, &cfg, Some(&req.model), &payload)
            .await?;
        Ok(LmpStreamHandle::new(protocol, inner))
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
                let model = format!("models/{}", req.model.trim_start_matches("models/"));
                let (operation, payload) = if req.input.len() == 1 {
                    ("embedContent", json!({"content": {"parts": [{"text": req.input[0]}]}}))
                } else {
                    let requests: Vec<_> = req.input.iter().map(|text| json!({"model": model, "content": {"parts": [{"text": text}]}})).collect();
                    ("batchEmbedContents", json!({"requests": requests}))
                };
                let url = format!("{base}/v1beta/{model}:{operation}");
                let resp = self.native.post_json_endpoint(&url, &headers, &payload, timeout, false).await?;
                let out = normalize_embedding_response(protocol, &resp.body)?;
                if out.embeddings.len() != req.input.len() {
                    return Err(LmpError::Decode("embedding count does not match request".into()));
                }
                Ok(out)
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
            | NativeProtocol::AwsBedrock
            | NativeProtocol::GoogleVertexAi => Err(LmpError::UnsupportedFeature {
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

/// Incremental LMP stream: pulls native frames, normalizes each one, and
/// yields unified events as soon as they are decodable.
///
/// Backpressure is consumer-driven: no socket reads happen until
/// [`LmpStreamHandle::next_event`] is called. A terminal failure is delivered
/// as one `LmpStreamEvent::Error` followed by `None`, so callers never mistake
/// a truncated stream for a clean end.
pub struct LmpStreamHandle {
    protocol: NativeProtocol,
    inner: EventStreamHandle,
    normalizer: StreamNormalizer,
    buffer: VecDeque<LmpStreamEvent>,
    failure: Option<LmpError>,
    finished: bool,
}

impl LmpStreamHandle {
    pub(crate) fn new(protocol: NativeProtocol, inner: EventStreamHandle) -> Self {
        Self {
            protocol,
            normalizer: StreamNormalizer::new(protocol),
            inner,
            buffer: VecDeque::new(),
            failure: None,
            finished: false,
        }
    }

    /// Protocol driving this stream (for diagnostics).
    pub fn protocol(&self) -> NativeProtocol {
        self.protocol
    }

    pub fn status(&self) -> u16 {
        self.inner.status()
    }

    pub fn headers(&self) -> &HashMap<String, String> {
        self.inner.headers()
    }

    /// Raw payload bytes consumed from upstream so far.
    pub fn bytes_read(&self) -> usize {
        self.inner.bytes_read()
    }

    /// Requests cancellation; pending and future reads fail with `cancelled`.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Shared cancellation flag, for language bindings that cancel by token.
    pub fn cancellation(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.inner.cancellation()
    }

    /// Wake handle for blocked readers (used by the C ABI cancel path).
    pub fn cancellation_notify(&self) -> std::sync::Arc<tokio::sync::Notify> {
        self.inner.cancellation_notify()
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// True once the terminal event has been delivered and drained.
    pub fn is_finished(&self) -> bool {
        self.finished && self.buffer.is_empty()
    }

    /// Error that terminated the stream, if any.
    pub fn failure(&self) -> Option<&LmpError> {
        self.failure.as_ref()
    }

    /// Returns the next unified event, or `None` once the stream is over.
    pub async fn next_event(&mut self) -> Result<Option<LmpStreamEvent>, LmpError> {
        loop {
            if let Some(event) = self.buffer.pop_front() {
                if matches!(event, LmpStreamEvent::Error { .. }) {
                    self.finished = true;
                }
                return Ok(Some(event));
            }
            if self.finished || self.failure.is_some() {
                return Ok(None);
            }
            if self.normalizer.is_finished() {
                self.finished = true;
                continue;
            }
            match self.inner.next_event().await {
                Ok(Some(event)) => {
                    let mut staged = Vec::new();
                    if let Err(err) = self.normalizer.push(&event, &mut staged) {
                        self.fail(err);
                    }
                    self.buffer.extend(staged);
                }
                Ok(None) => {
                    let mut staged = Vec::new();
                    self.normalizer.finish(&mut staged);
                    self.buffer.extend(staged);
                    if self.buffer.is_empty() {
                        self.finished = true;
                    }
                }
                Err(err) => self.fail(err.into()),
            }
        }
    }

    fn fail(&mut self, err: LmpError) {
        self.buffer.push_back(LmpStreamEvent::Error {
            code: err.code().to_string(),
            message: err.to_string(),
        });
        self.failure = Some(err);
        self.finished = true;
    }

    /// Drains the stream into memory (compatibility helper).
    pub async fn collect(&mut self) -> Result<Vec<LmpStreamEvent>, LmpError> {
        let mut events = Vec::new();
        while let Some(event) = self.next_event().await? {
            events.push(event);
        }
        Ok(events)
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
        NativeProtocol::GoogleGemini => {
            let rows = raw["embeddings"].as_array().map(Vec::as_slice)
                .unwrap_or_else(|| std::slice::from_ref(&raw["embedding"]));
            let embeddings = rows.iter().map(|row| {
                let values = row["values"].as_array().filter(|values| !values.is_empty())
                    .ok_or_else(|| LmpError::Decode("missing embedding.values".into()))?;
                values.iter().map(|value| value.as_f64().ok_or_else(|| LmpError::Decode("invalid embedding component".into()))).collect()
            }).collect::<Result<_, _>>()?;
            Ok(EmbeddingResponse { embeddings, usage: Usage::default(), raw: raw.clone() })
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
