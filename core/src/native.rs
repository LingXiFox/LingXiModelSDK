use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::errors::NativeError;
use crate::eventstream::{EventStream, MEDIA_TYPE};
use crate::registry::{validate_endpoint_url, AuthConfig, NativeProtocol, ProviderRegistry};
use crate::sigv4::{current_amz_date, sign_sigv4, SigV4Input};
use crate::sse::{NdjsonParser, SseEvent, SseParser};

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct NativeRequestConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    pub timeout: Duration,
    pub allow_insecure_http: bool,
}

impl std::fmt::Debug for NativeRequestConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeRequestConfig")
            .field("timeout", &self.timeout)
            .field("allow_insecure_http", &self.allow_insecure_http)
            .finish_non_exhaustive()
    }
}

impl NativeRequestConfig {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key,
            headers: HashMap::new(),
            timeout: Duration::from_secs(60),
            allow_insecure_http: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeJsonResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Value,
    pub raw_bytes: Vec<u8>,
}

pub struct NativeClient {
    http: reqwest::Client,
}

impl Default for NativeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build redirect-disabled HTTP client");
        Self { http }
    }

    pub async fn openai_chat_completions(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let url = join_path(&cfg.base_url, "/chat/completions");
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_openai_chat_completions(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let url = join_path(&cfg.base_url, "/chat/completions");
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        let body = with_stream_true(payload);
        self.post_sse(&url, &headers, &body, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn openai_responses(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let url = join_path(&cfg.base_url, "/responses");
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_openai_responses(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let url = join_path(&cfg.base_url, "/responses");
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        let body = with_stream_true(payload);
        self.post_sse(&url, &headers, &body, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn anthropic_messages(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let url = join_anthropic_messages(&cfg.base_url);
        let mut headers = cfg.headers.clone();
        apply_header_auth(&mut headers, "x-api-key", cfg.api_key.as_deref())?;
        headers
            .entry("anthropic-version".to_string())
            .or_insert_with(|| "2023-06-01".to_string());
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_anthropic_messages(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let url = join_anthropic_messages(&cfg.base_url);
        let mut headers = cfg.headers.clone();
        apply_header_auth(&mut headers, "x-api-key", cfg.api_key.as_deref())?;
        headers
            .entry("anthropic-version".to_string())
            .or_insert_with(|| "2023-06-01".to_string());
        let body = with_stream_true(payload);
        self.post_sse(&url, &headers, &body, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn google_gemini_generate(
        &self,
        cfg: &NativeRequestConfig,
        model: &str,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let url = format!(
            "{}/v1beta/models/{}:generateContent",
            cfg.base_url.trim_end_matches('/'),
            model
        );
        let mut headers = cfg.headers.clone();
        apply_header_auth(&mut headers, "x-goog-api-key", cfg.api_key.as_deref())?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_google_gemini_generate(
        &self,
        cfg: &NativeRequestConfig,
        model: &str,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
            cfg.base_url.trim_end_matches('/'),
            model
        );
        let mut headers = cfg.headers.clone();
        apply_header_auth(&mut headers, "x-goog-api-key", cfg.api_key.as_deref())?;
        self.post_sse(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn aws_bedrock_converse(
        &self,
        cfg: &NativeRequestConfig,
        model: &str,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let uri = format!("/model/{}/converse", aws_encode_path(model, false));
        let url = format!("{}{uri}", cfg.base_url.trim_end_matches('/'));
        let mut headers = cfg.headers.clone();
        apply_bedrock_auth(&url, payload, cfg.api_key.as_deref(), &mut headers)?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_aws_bedrock_converse(
        &self,
        cfg: &NativeRequestConfig,
        model: &str,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let uri = format!("/model/{}/converse-stream", aws_encode_path(model, false));
        let url = format!("{}{uri}", cfg.base_url.trim_end_matches('/'));
        let mut headers = cfg.headers.clone();
        apply_bedrock_auth(&url, payload, cfg.api_key.as_deref(), &mut headers)?;
        self.post_ndjson(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http, true)
            .await
    }

    pub async fn google_vertex_generate(
        &self,
        cfg: &NativeRequestConfig,
        model: &str,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let base = cfg.base_url.trim_end_matches('/');
        let url = if base.contains("/models") {
            format!("{base}/{model}:generateContent")
        } else {
            format!("{base}/v1/publishers/google/models/{model}:generateContent")
        };
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_google_vertex_generate(
        &self,
        cfg: &NativeRequestConfig,
        model: &str,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let base = cfg.base_url.trim_end_matches('/');
        let url = if base.contains("/models") {
            format!("{base}/{model}:streamGenerateContent?alt=sse")
        } else {
            format!("{base}/v1/publishers/google/models/{model}:streamGenerateContent?alt=sse")
        };
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        self.post_sse(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn azure_openai_chat_completions(
        &self,
        cfg: &NativeRequestConfig,
        deployment: &str,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let api_version = cfg
            .headers
            .get("x-azure-api-version")
            .map(String::as_str)
            .unwrap_or("2024-10-21");
        let url = format!(
            "{}/openai/deployments/{deployment}/chat/completions?api-version={api_version}",
            cfg.base_url.trim_end_matches('/')
        );
        let mut headers = cfg.headers.clone();
        apply_header_auth(&mut headers, "api-key", cfg.api_key.as_deref())?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_azure_openai_chat_completions(
        &self,
        cfg: &NativeRequestConfig,
        deployment: &str,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let api_version = cfg
            .headers
            .get("x-azure-api-version")
            .map(String::as_str)
            .unwrap_or("2024-10-21");
        let url = format!(
            "{}/openai/deployments/{deployment}/chat/completions?api-version={api_version}",
            cfg.base_url.trim_end_matches('/')
        );
        let mut headers = cfg.headers.clone();
        apply_header_auth(&mut headers, "api-key", cfg.api_key.as_deref())?;
        let body = with_stream_true(payload);
        self.post_sse(&url, &headers, &body, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn ollama_chat(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let url = join_path(&cfg.base_url, "/api/chat");
        let mut body = payload.clone();
        if let Some(obj) = body.as_object_mut() {
            obj.entry("stream".to_string()).or_insert(Value::Bool(false));
        }
        self.post_json(&url, &cfg.headers, &body, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_ollama_chat(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let url = join_path(&cfg.base_url, "/api/chat");
        let body = with_stream_true(payload);
        self.post_ndjson(&url, &cfg.headers, &body, cfg.timeout, cfg.allow_insecure_http, false)
            .await
    }

    pub async fn cohere_chat(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let url = join_path(&cfg.base_url, "/v2/chat");
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        self.post_json(&url, &headers, payload, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn stream_cohere_chat(
        &self,
        cfg: &NativeRequestConfig,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let url = join_path(&cfg.base_url, "/v2/chat");
        let mut headers = cfg.headers.clone();
        apply_bearer(&mut headers, cfg.api_key.as_deref())?;
        let body = with_stream_true(payload);
        self.post_sse(&url, &headers, &body, cfg.timeout, cfg.allow_insecure_http)
            .await
    }

    pub async fn post_json_endpoint(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        payload: &Value,
        timeout: Duration,
        allow_insecure_http: bool,
    ) -> Result<NativeJsonResponse, NativeError> {
        self.post_json(url, headers, payload, timeout, allow_insecure_http)
            .await
    }

    /// Dispatches a native request using the declarative ProviderRegistry.
    pub async fn send_registered_json(
        &self,
        registry: &ProviderRegistry,
        provider_id: &str,
        protocol: NativeProtocol,
        model_id: Option<&str>,
        api_key: Option<String>,
        endpoint_override: Option<&str>,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        let (entry, base) =
            registry.resolve_endpoint(provider_id, protocol, model_id, endpoint_override, false)?;
        let mut headers = entry.default_headers.clone();
        if let Some(ov) = model_id.and_then(|mid| entry.model_overrides.get(mid)) {
            for (k, v) in &ov.default_headers {
                headers.insert(k.clone(), v.clone());
            }
        }
        apply_registry_auth(&mut headers, &entry.auth, api_key.as_deref())?;
        let merged_payload = merge_extra_params(payload, &entry.extra_params);
        let cfg = NativeRequestConfig {
            base_url: base,
            api_key,
            headers,
            timeout: Duration::from_secs(60),
            allow_insecure_http: false,
        };
        self.request_json(protocol, &cfg, model_id, &merged_payload).await
    }

    pub async fn request_json(
        &self,
        protocol: NativeProtocol,
        cfg: &NativeRequestConfig,
        model_id: Option<&str>,
        payload: &Value,
    ) -> Result<NativeJsonResponse, NativeError> {
        match protocol {
            NativeProtocol::OpenaiChatCompletions => {
                self.openai_chat_completions(cfg, payload).await
            }
            NativeProtocol::OpenaiResponses => self.openai_responses(cfg, payload).await,
            NativeProtocol::AnthropicMessages => {
                self.anthropic_messages(cfg, payload).await
            }
            NativeProtocol::GoogleGemini => {
                let model = model_id.ok_or_else(|| {
                    NativeError::Registry("google_gemini requires a model_id".into())
                })?;
                self.google_gemini_generate(cfg, model, payload)
                    .await
            }
            NativeProtocol::AwsBedrock => {
                let model = model_id.ok_or_else(|| {
                    NativeError::Registry("aws_bedrock requires a model_id".into())
                })?;
                self.aws_bedrock_converse(cfg, model, payload).await
            }
            NativeProtocol::GoogleVertexAi => {
                let model = model_id.ok_or_else(|| {
                    NativeError::Registry("google_vertex_ai requires a model_id".into())
                })?;
                self.google_vertex_generate(cfg, model, payload)
                    .await
            }
            NativeProtocol::AzureOpenai => {
                let deployment = model_id.ok_or_else(|| {
                    NativeError::Registry("azure_openai requires a model_id/deployment".into())
                })?;
                self.azure_openai_chat_completions(cfg, deployment, payload)
                    .await
            }
            NativeProtocol::Ollama => self.ollama_chat(cfg, payload).await,
            NativeProtocol::Cohere => self.cohere_chat(cfg, payload).await,
        }
    }

    pub async fn request_stream(
        &self,
        protocol: NativeProtocol,
        cfg: &NativeRequestConfig,
        model_id: Option<&str>,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        let model_required = || model_id.ok_or_else(|| NativeError::Registry(format!("{} requires a model_id", protocol.as_str())));
        match protocol {
            NativeProtocol::OpenaiChatCompletions => {
                self
                    .stream_openai_chat_completions(cfg, payload)
                    .await
            }
            NativeProtocol::OpenaiResponses => {
                self.stream_openai_responses(cfg, payload).await
            }
            NativeProtocol::AnthropicMessages => {
                self
                    .stream_anthropic_messages(cfg, payload)
                    .await
            }
            NativeProtocol::GoogleGemini => {
                self
                    .stream_google_gemini_generate(cfg, model_required()?, payload)
                    .await
            }
            NativeProtocol::AwsBedrock => {
                self
                    .stream_aws_bedrock_converse(cfg, model_required()?, payload)
                    .await
            }
            NativeProtocol::GoogleVertexAi => {
                self
                    .stream_google_vertex_generate(cfg, model_required()?, payload)
                    .await
            }
            NativeProtocol::AzureOpenai => {
                self
                    .stream_azure_openai_chat_completions(cfg, model_required()?, payload)
                    .await
            }
            NativeProtocol::Ollama => self.stream_ollama_chat(cfg, payload).await,
            NativeProtocol::Cohere => self.stream_cohere_chat(cfg, payload).await,
        }
    }

    async fn post_json(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        payload: &Value,
        timeout: Duration,
        allow_insecure_http: bool,
    ) -> Result<NativeJsonResponse, NativeError> {
        validate_endpoint_url(url, allow_insecure_http)?;
        let body_bytes = serde_json::to_vec(payload)
            .map_err(|e| NativeError::Transport(format!("failed to encode JSON: {e}")))?;
        let mut req = self
            .http
            .post(url)
            .timeout(timeout)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body_bytes);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let mut resp = req
            .send()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let resp_headers = collect_headers(resp.headers());
        let raw_bytes = read_bounded(&mut resp).await?;
        if !(200..=299).contains(&status) {
            return Err(NativeError::HttpStatus {
                status,
                body: raw_bytes,
                headers: resp_headers,
            });
        }
        let body: Value = serde_json::from_slice(&raw_bytes)
            .map_err(|e| NativeError::Transport(format!("invalid response JSON: {e}")))?;
        Ok(NativeJsonResponse {
            status,
            headers: resp_headers,
            body,
            raw_bytes,
        })
    }

    async fn post_sse(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        payload: &Value,
        timeout: Duration,
        allow_insecure_http: bool,
    ) -> Result<Vec<SseEvent>, NativeError> {
        validate_endpoint_url(url, allow_insecure_http)?;
        let body_bytes = serde_json::to_vec(payload)
            .map_err(|e| NativeError::Transport(format!("failed to encode JSON: {e}")))?;
        let mut req = self
            .http
            .post(url)
            .timeout(timeout)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .body(body_bytes);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let mut resp = req
            .send()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let resp_headers = collect_headers(resp.headers());
        if !(200..=299).contains(&status) {
            let raw = read_bounded(&mut resp).await?;
            return Err(NativeError::HttpStatus {
                status,
                body: raw,
                headers: resp_headers,
            });
        }
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        let mut total_bytes = 0usize;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?
        {
            total_bytes += chunk.len();
            if total_bytes > MAX_RESPONSE_BYTES {
                return Err(NativeError::Transport(format!(
                    "stream exceeded {MAX_RESPONSE_BYTES} byte limit"
                )));
            }
            events.extend(parser.push(&chunk[..]));
        }
        events.extend(parser.finish());
        Ok(events)
    }

    async fn post_ndjson(
        &self,
        url: &str,
        headers: &HashMap<String, String>,
        payload: &Value,
        timeout: Duration,
        allow_insecure_http: bool,
        bedrock: bool,
    ) -> Result<Vec<SseEvent>, NativeError> {
        validate_endpoint_url(url, allow_insecure_http)?;
        let body_bytes = serde_json::to_vec(payload)
            .map_err(|e| NativeError::Transport(format!("failed to encode JSON: {e}")))?;
        let mut req = self
            .http
            .post(url)
            .timeout(timeout)
            .header("Content-Type", "application/json")
            .header("Accept", if bedrock { MEDIA_TYPE } else { "application/x-ndjson, application/json" })
            .body(body_bytes);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let mut resp = req
            .send()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let resp_headers = collect_headers(resp.headers());
        if !(200..=299).contains(&status) {
            let raw = read_bounded(&mut resp).await?;
            return Err(NativeError::HttpStatus {
                status,
                body: raw,
                headers: resp_headers,
            });
        }
        let content_type = resp.headers().get("content-type")
            .and_then(|value| value.to_str().ok()).unwrap_or("")
            .split(';').next().unwrap_or("").trim();
        let binary = bedrock && content_type.eq_ignore_ascii_case(MEDIA_TYPE);
        if bedrock && !binary && content_type != "application/x-ndjson" {
            return Err(NativeError::StreamParse("unexpected Bedrock response content type".into()));
        }
        let mut event_stream = EventStream::default();
        let mut parser = NdjsonParser::new();
        let mut events = Vec::new();
        let mut total_bytes = 0usize;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?
        {
            total_bytes += chunk.len();
            if total_bytes > MAX_RESPONSE_BYTES {
                return Err(NativeError::Transport(format!(
                    "stream exceeded {MAX_RESPONSE_BYTES} byte limit"
                )));
            }
            if binary {
                events.extend(event_stream.push(&chunk)?);
            } else {
                events.extend(parser.push(&chunk));
            }
        }
        if binary {
            event_stream.finish()?;
        } else {
            events.extend(parser.finish());
        }
        Ok(events)
    }
}

async fn read_bounded(resp: &mut reqwest::Response) -> Result<Vec<u8>, NativeError> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| NativeError::Transport(e.to_string()))? {
        if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
            return Err(NativeError::Transport(format!("response exceeded {MAX_RESPONSE_BYTES} byte limit")));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn join_path(base: &str, suffix: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), suffix)
}

fn join_anthropic_messages(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

fn with_stream_true(payload: &Value) -> Value {
    let mut cloned = payload.clone();
    if let Some(obj) = cloned.as_object_mut() {
        obj.insert("stream".to_string(), Value::Bool(true));
    }
    cloned
}

fn merge_extra_params(payload: &Value, extras: &Map<String, Value>) -> Value {
    if extras.is_empty() {
        return payload.clone();
    }
    let mut obj = payload.as_object().cloned().unwrap_or_default();
    for (k, v) in extras {
        obj.entry(k.clone()).or_insert_with(|| v.clone());
    }
    Value::Object(obj)
}

fn apply_bearer(headers: &mut HashMap<String, String>, key: Option<&str>) -> Result<(), NativeError> {
    let key = key.filter(|s| !s.is_empty()).ok_or_else(|| {
        NativeError::AuthMissing("bearer API key is required".into())
    })?;
    headers
        .entry("Authorization".to_string())
        .or_insert_with(|| format!("Bearer {key}"));
    Ok(())
}

fn apply_header_auth(
    headers: &mut HashMap<String, String>,
    header_name: &str,
    key: Option<&str>,
) -> Result<(), NativeError> {
    let key = key.filter(|s| !s.is_empty()).ok_or_else(|| {
        NativeError::AuthMissing(format!("header '{header_name}' API key is required"))
    })?;
    headers
        .entry(header_name.to_string())
        .or_insert_with(|| key.to_string());
    Ok(())
}

fn aws_encode_path(value: &str, preserve_slash: bool) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) || (preserve_slash && byte == b'/') {
            encoded.push(char::from(byte));
        } else {
            write!(&mut encoded, "%{byte:02X}").unwrap();
        }
    }
    encoded
}

fn apply_bedrock_auth(
    url: &str,
    payload: &Value,
    api_key: Option<&str>,
    headers: &mut HashMap<String, String>,
) -> Result<(), NativeError> {
    if headers.contains_key("Authorization") || headers.contains_key("authorization") {
        return Ok(());
    }
    let key = api_key.filter(|s| !s.is_empty()).ok_or_else(|| {
        NativeError::AuthMissing("AWS Bedrock requires 'ACCESS_KEY:SECRET_KEY[:SESSION_TOKEN]'".into())
    })?;
    let parts: Vec<&str> = key.split(':').collect();
    if parts.len() < 2 {
        // Fallback: bearer token for Bedrock API keys / proxy relays
        return apply_bearer(headers, Some(key));
    }
    let access_key = parts[0];
    let secret_key = parts[1];
    let session_token = parts.get(2).copied();
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| NativeError::InvalidEndpoint(format!("invalid URL '{url}': {e}")))?;
    let host = parsed.host_str().unwrap_or("bedrock-runtime.us-east-1.amazonaws.com");
    let host_hdr = match parsed.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    };
    let region = headers.get("x-aws-region").map(String::as_str).or_else(|| {
        let tail = host.strip_prefix("bedrock-runtime.")?;
        let (region, domain) = tail.split_once('.')?;
        matches!(domain, "amazonaws.com" | "amazonaws.com.cn" | "api.aws").then_some(region)
    }).unwrap_or("us-east-1");
    let canonical_uri = aws_encode_path(parsed.path(), true);
    let amz_date = current_amz_date();
    let body_bytes = serde_json::to_vec(payload)
        .map_err(|e| NativeError::Transport(format!("failed to encode JSON: {e}")))?;
    for (k, v) in sign_sigv4(&SigV4Input {
        method: "POST",
        canonical_uri: &canonical_uri,
        canonical_query: "",
        host: &host_hdr,
        region,
        service: "bedrock",
        amz_date: &amz_date,
        access_key,
        secret_key,
        session_token,
        payload: &body_bytes,
    }) {
        headers.insert(k, v);
    }
    Ok(())
}

fn apply_registry_auth(
    headers: &mut HashMap<String, String>,
    auth: &AuthConfig,
    key: Option<&str>,
) -> Result<(), NativeError> {
    match auth.auth_type.as_str() {
        "none" => Ok(()),
        "bearer" => apply_bearer(headers, key),
        "x-api-key" => apply_header_auth(headers, "x-api-key", key),
        "api-key" => apply_header_auth(headers, "api-key", key),
        "x-goog-api-key" => apply_header_auth(headers, "x-goog-api-key", key),
        "aws_sigv4" => {
            if key.filter(|s| !s.is_empty()).is_none() {
                return Err(NativeError::AuthMissing("aws_sigv4 credentials required".into()));
            }
            Ok(())
        }
        "custom_header" => {
            let name = auth.header_name.as_deref().unwrap_or("Authorization");
            let val = key.filter(|s| !s.is_empty()).ok_or_else(|| {
                NativeError::AuthMissing(format!("custom header '{name}' key required"))
            })?;
            let formatted = match &auth.prefix {
                Some(prefix) => format!("{prefix}{val}"),
                None => val.to_string(),
            };
            headers.entry(name.to_string()).or_insert(formatted);
            Ok(())
        }
        other => Err(NativeError::Registry(format!(
            "unsupported auth type '{other}'"
        ))),
    }
}

fn collect_headers(map: &reqwest::header::HeaderMap) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in map {
        if let Ok(val) = v.to_str() {
            out.insert(k.as_str().to_lowercase(), val.to_string());
        }
    }
    out
}
