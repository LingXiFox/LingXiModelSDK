use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::errors::NativeError;
use crate::eventstream::{EventStream, MEDIA_TYPE};
use crate::registry::{validate_endpoint_url, AuthConfig, NativeProtocol, ProviderRegistry};
use crate::sigv4::{current_amz_date, sign_sigv4, SigV4Input};
use crate::sse::{NdjsonParser, SseEvent, SseParser};
use crate::stream::{EventStreamHandle, StreamDecoder};
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct NativeRequestConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    pub timeout: Duration,
    /// Maximum silence between stream chunks before the read fails.
    pub stream_idle_timeout: Duration,
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
            stream_idle_timeout: Duration::from_secs(120),
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
        self.open_stream_events(NativeProtocol::OpenaiChatCompletions, cfg, None, payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::OpenaiResponses, cfg, None, payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::AnthropicMessages, cfg, None, payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::GoogleGemini, cfg, Some(model), payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::AwsBedrock, cfg, Some(model), payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::GoogleVertexAi, cfg, Some(model), payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::AzureOpenai, cfg, Some(deployment), payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::Ollama, cfg, None, payload)
            .await?
            .collect()
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
        self.open_stream_events(NativeProtocol::Cohere, cfg, None, payload)
            .await?
            .collect()
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
            stream_idle_timeout: Duration::from_secs(120),
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
                    NativeError::InvalidRequest("google_gemini requires a model_id".into())
                })?;
                self.google_gemini_generate(cfg, model, payload)
                    .await
            }
            NativeProtocol::AwsBedrock => {
                let model = model_id.ok_or_else(|| {
                    NativeError::InvalidRequest("aws_bedrock requires a model_id".into())
                })?;
                self.aws_bedrock_converse(cfg, model, payload).await
            }
            NativeProtocol::GoogleVertexAi => {
                let model = model_id.ok_or_else(|| {
                    NativeError::InvalidRequest("google_vertex_ai requires a model_id".into())
                })?;
                self.google_vertex_generate(cfg, model, payload)
                    .await
            }
            NativeProtocol::AzureOpenai => {
                let deployment = model_id.ok_or_else(|| {
                    NativeError::InvalidRequest("azure_openai requires a model_id/deployment".into())
                })?;
                self.azure_openai_chat_completions(cfg, deployment, payload)
                    .await
            }
            NativeProtocol::Ollama => self.ollama_chat(cfg, payload).await,
            NativeProtocol::Cohere => self.cohere_chat(cfg, payload).await,
        }
    }


    /// Opens a stream without buffering. Use this when the caller needs to see
    /// events before the upstream response finishes.
    pub async fn open_stream(
        &self,
        protocol: NativeProtocol,
        cfg: &NativeRequestConfig,
        model_id: Option<&str>,
        payload: &Value,
    ) -> Result<EventStreamHandle, NativeError> {
        self.open_stream_events(protocol, cfg, model_id, payload).await
    }

    /// Buffered convenience wrapper over [`NativeClient::open_stream`].
    pub async fn request_stream(
        &self,
        protocol: NativeProtocol,
        cfg: &NativeRequestConfig,
        model_id: Option<&str>,
        payload: &Value,
    ) -> Result<Vec<SseEvent>, NativeError> {
        self.open_stream_events(protocol, cfg, model_id, payload)
            .await?
            .collect()
            .await
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

    /// Opens an incremental stream: the request is sent and the response
    /// headers are validated here, but the body is decoded lazily by
    /// [`EventStreamHandle::next_event`]. Callers that want the old buffered
    /// behaviour call `.collect().await` on the returned handle.
    pub async fn open_stream_events(
        &self,
        protocol: NativeProtocol,
        cfg: &NativeRequestConfig,
        model_id: Option<&str>,
        payload: &Value,
    ) -> Result<EventStreamHandle, NativeError> {
        let (url, headers, body, framing) = stream_target(protocol, cfg, model_id, payload)?;
        validate_endpoint_url(&url, cfg.allow_insecure_http)?;
        let body_bytes = serde_json::to_vec(&body)
            .map_err(|e| NativeError::Transport(format!("failed to encode JSON: {e}")))?;
        let accept = match framing {
            StreamFraming::AmazonEventStream => MEDIA_TYPE,
            StreamFraming::Ndjson => "application/x-ndjson, application/json",
            StreamFraming::Sse => "text/event-stream",
        };
        let mut req = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Accept", accept)
            .body(body_bytes);
        if !matches!(framing, StreamFraming::AmazonEventStream) {
            req = req.timeout(cfg.timeout);
        }
        for (k, v) in &headers {
            req = req.header(k, v);
        }
        let mut resp = req
            .send()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        if !(200..=299).contains(&status) {
            let headers = collect_headers(resp.headers());
            let body = read_bounded(&mut resp).await?;
            return Err(NativeError::HttpStatus {
                status,
                body,
                headers,
            });
        }
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let decoder = match framing {
            StreamFraming::Sse => StreamDecoder::Sse(SseParser::new()),
            StreamFraming::Ndjson => StreamDecoder::Ndjson(NdjsonParser::new()),
            StreamFraming::AmazonEventStream => {
                if content_type.eq_ignore_ascii_case(MEDIA_TYPE) {
                    StreamDecoder::AmazonEventStream(EventStream::default())
                } else if content_type.eq_ignore_ascii_case("application/json")
                    || content_type.eq_ignore_ascii_case("application/x-ndjson")
                {
                    // Bedrock-compatible relays may answer with plain JSON lines.
                    StreamDecoder::Ndjson(NdjsonParser::new())
                } else {
                    return Err(NativeError::StreamParse(format!(
                        "unexpected Bedrock response content type: {content_type}"
                    )));
                }
            }
        };
        Ok(EventStreamHandle::new(
            resp,
            decoder,
            cfg.stream_idle_timeout,
        ))
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


/// Wire framing used by an upstream streaming endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamFraming {
    Sse,
    Ndjson,
    AmazonEventStream,
}

/// Resolves the streaming URL, auth headers, request body and framing for a
/// protocol. Single source of truth for both the incremental handle and the
/// buffered wrappers, so the two can never drift apart.
pub(crate) fn stream_target(
    protocol: NativeProtocol,
    cfg: &NativeRequestConfig,
    model_id: Option<&str>,
    payload: &Value,
) -> Result<(String, HashMap<String, String>, Value, StreamFraming), NativeError> {
    let require_model = || {
        model_id.ok_or_else(|| {
            NativeError::InvalidRequest(format!("{} requires a model_id", protocol.as_str()))
        })
    };
    let mut headers = cfg.headers.clone();
    let url;
    let body;
    let framing;
    match protocol {
        NativeProtocol::OpenaiChatCompletions => {
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            url = join_path(&cfg.base_url, "/chat/completions");
            body = with_stream_true(payload);
            framing = StreamFraming::Sse;
        }
        NativeProtocol::OpenaiResponses => {
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            url = join_path(&cfg.base_url, "/responses");
            body = with_stream_true(payload);
            framing = StreamFraming::Sse;
        }
        NativeProtocol::AnthropicMessages => {
            apply_header_auth(&mut headers, "x-api-key", cfg.api_key.as_deref())?;
            headers
                .entry("anthropic-version".to_string())
                .or_insert_with(|| "2023-06-01".to_string());
            url = join_anthropic_messages(&cfg.base_url);
            body = with_stream_true(payload);
            framing = StreamFraming::Sse;
        }
        NativeProtocol::GoogleGemini => {
            let model = require_model()?;
            apply_header_auth(&mut headers, "x-goog-api-key", cfg.api_key.as_deref())?;
            url = format!(
                "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
                cfg.base_url.trim_end_matches('/'),
                model
            );
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::AwsBedrock => {
            let model = require_model()?;
            let uri = format!("/model/{}/converse-stream", aws_encode_path(model, false));
            url = format!("{}{uri}", cfg.base_url.trim_end_matches('/'));
            apply_bedrock_auth(&url, payload, cfg.api_key.as_deref(), &mut headers)?;
            body = payload.clone();
            framing = StreamFraming::AmazonEventStream;
        }
        NativeProtocol::GoogleVertexAi => {
            let model = require_model()?;
            let base = cfg.base_url.trim_end_matches('/');
            url = if base.contains("/models") {
                format!("{base}/{model}:streamGenerateContent?alt=sse")
            } else {
                format!("{base}/v1/publishers/google/models/{model}:streamGenerateContent?alt=sse")
            };
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::AzureOpenai => {
            let deployment = require_model()?;
            let api_version = cfg
                .headers
                .get("x-azure-api-version")
                .map(String::as_str)
                .unwrap_or("2024-10-21");
            url = format!(
                "{}/openai/deployments/{deployment}/chat/completions?api-version={api_version}",
                cfg.base_url.trim_end_matches('/')
            );
            apply_header_auth(&mut headers, "api-key", cfg.api_key.as_deref())?;
            body = with_stream_true(payload);
            framing = StreamFraming::Sse;
        }
        NativeProtocol::Ollama => {
            url = join_path(&cfg.base_url, "/api/chat");
            body = with_stream_true(payload);
            framing = StreamFraming::Ndjson;
        }
        NativeProtocol::Cohere => {
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            url = join_path(&cfg.base_url, "/v2/chat");
            body = with_stream_true(payload);
            framing = StreamFraming::Sse;
        }
    }
    Ok((url, headers, body, framing))
}


/// Resolved wire target for one native request: where to send it, with which
/// headers, and which framing the response uses.
pub(crate) struct PreparedTarget {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Value,
    pub framing: StreamFraming,
}

impl StreamFraming {
    /// Stable wire name handed to foreign-language SDKs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sse => "sse",
            Self::Ndjson => "ndjson",
            Self::AmazonEventStream => "amazon_event_stream",
        }
    }
}

/// Single source of truth for non-streaming URL, auth headers and body per
/// protocol. Foreign SDKs reach this through lingxi_native_prepare, so they
/// never re-implement endpoint joining or AWS SigV4.
pub(crate) fn native_target(
    protocol: NativeProtocol,
    cfg: &NativeRequestConfig,
    model_id: Option<&str>,
    payload: &Value,
) -> Result<PreparedTarget, NativeError> {
    let require_model = |what: &str| {
        model_id.map(str::to_string).ok_or_else(|| {
            NativeError::InvalidRequest(format!("{} requires a {what}", protocol.as_str()))
        })
    };
    let mut headers = cfg.headers.clone();
    let (url, body, framing);
    match protocol {
        NativeProtocol::OpenaiChatCompletions => {
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            url = join_path(&cfg.base_url, "/chat/completions");
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::OpenaiResponses => {
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            url = join_path(&cfg.base_url, "/responses");
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::AnthropicMessages => {
            apply_header_auth(&mut headers, "x-api-key", cfg.api_key.as_deref())?;
            headers
                .entry("anthropic-version".to_string())
                .or_insert_with(|| "2023-06-01".to_string());
            url = join_anthropic_messages(&cfg.base_url);
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::GoogleGemini => {
            let model = require_model("model_id")?;
            apply_header_auth(&mut headers, "x-goog-api-key", cfg.api_key.as_deref())?;
            url = format!(
                "{}/v1beta/models/{}:generateContent",
                cfg.base_url.trim_end_matches('/'),
                model
            );
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::AwsBedrock => {
            let model = require_model("model_id")?;
            let uri = format!("/model/{}/converse", aws_encode_path(&model, false));
            url = format!("{}{uri}", cfg.base_url.trim_end_matches('/'));
            apply_bedrock_auth(&url, payload, cfg.api_key.as_deref(), &mut headers)?;
            body = payload.clone();
            framing = StreamFraming::AmazonEventStream;
        }
        NativeProtocol::GoogleVertexAi => {
            let model = require_model("model_id")?;
            let base = cfg.base_url.trim_end_matches('/');
            url = if base.contains("/models") {
                format!("{base}/{model}:generateContent")
            } else {
                format!("{base}/v1/publishers/google/models/{model}:generateContent")
            };
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::AzureOpenai => {
            let deployment = require_model("deployment")?;
            let api_version = cfg
                .headers
                .get("x-azure-api-version")
                .map(String::as_str)
                .unwrap_or("2024-10-21");
            url = format!(
                "{}/openai/deployments/{deployment}/chat/completions?api-version={api_version}",
                cfg.base_url.trim_end_matches('/')
            );
            apply_header_auth(&mut headers, "api-key", cfg.api_key.as_deref())?;
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
        NativeProtocol::Ollama => {
            url = join_path(&cfg.base_url, "/api/chat");
            let mut body_value = payload.clone();
            if let Some(obj) = body_value.as_object_mut() {
                obj.entry("stream".to_string()).or_insert(Value::Bool(false));
            }
            body = body_value;
            framing = StreamFraming::Ndjson;
        }
        NativeProtocol::Cohere => {
            apply_bearer(&mut headers, cfg.api_key.as_deref())?;
            url = join_path(&cfg.base_url, "/v2/chat");
            body = payload.clone();
            framing = StreamFraming::Sse;
        }
    }
    Ok(PreparedTarget {
        url,
        headers,
        body,
        framing,
    })
}

/// Resolves the wire target for a native request, either streaming or not.
/// Foreign SDKs call this through the C ABI so endpoint joining, auth header
/// construction and Bedrock SigV4 signing live in exactly one place.
pub(crate) fn prepare_target(
    protocol: NativeProtocol,
    cfg: &NativeRequestConfig,
    model_id: Option<&str>,
    payload: &Value,
    stream: bool,
) -> Result<PreparedTarget, NativeError> {
    if stream {
        let (url, headers, body, framing) = stream_target(protocol, cfg, model_id, payload)?;
        validate_endpoint_url(&url, cfg.allow_insecure_http)?;
        Ok(PreparedTarget {
            url,
            headers,
            body,
            framing,
        })
    } else {
        let target = native_target(protocol, cfg, model_id, payload)?;
        validate_endpoint_url(&target.url, cfg.allow_insecure_http)?;
        Ok(target)
    }
}
