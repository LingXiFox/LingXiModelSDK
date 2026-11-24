use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Map, Value};

use crate::errors::NativeError;
use crate::registry::{validate_endpoint_url, AuthConfig, NativeProtocol, ProviderRegistry};
use crate::sse::{SseEvent, SseParser};

#[derive(Debug, Clone)]
pub struct NativeRequestConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    pub timeout: Duration,
    pub allow_insecure_http: bool,
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
        Self {
            http: reqwest::Client::new(),
        }
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
        match protocol {
            NativeProtocol::OpenaiChatCompletions => {
                self.openai_chat_completions(&cfg, &merged_payload).await
            }
            NativeProtocol::OpenaiResponses => self.openai_responses(&cfg, &merged_payload).await,
            NativeProtocol::AnthropicMessages => {
                self.anthropic_messages(&cfg, &merged_payload).await
            }
            NativeProtocol::GoogleGemini => {
                let model = model_id.ok_or_else(|| {
                    NativeError::Registry("google_gemini requires a model_id".into())
                })?;
                self.google_gemini_generate(&cfg, model, &merged_payload)
                    .await
            }
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
        let resp = req
            .send()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let resp_headers = collect_headers(resp.headers());
        let raw_bytes = resp
            .bytes()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?
            .to_vec();
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
            let raw = resp.bytes().await.unwrap_or_default().to_vec();
            return Err(NativeError::HttpStatus {
                status,
                body: raw,
                headers: resp_headers,
            });
        }
        let mut parser = SseParser::new();
        let mut events = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| NativeError::Transport(e.to_string()))?
        {
            events.extend(parser.push(&chunk[..]));
        }
        events.extend(parser.finish());
        Ok(events)
    }
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

fn apply_registry_auth(
    headers: &mut HashMap<String, String>,
    auth: &AuthConfig,
    key: Option<&str>,
) -> Result<(), NativeError> {
    match auth.auth_type.as_str() {
        "none" => Ok(()),
        "bearer" => apply_bearer(headers, key),
        "x-api-key" => apply_header_auth(headers, "x-api-key", key),
        "x-goog-api-key" => apply_header_auth(headers, "x-goog-api-key", key),
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
