use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::errors::NativeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProtocol {
    OpenaiChatCompletions,
    OpenaiResponses,
    AnthropicMessages,
    GoogleGemini,
    AwsBedrock,
    GoogleVertexAi,
    AzureOpenai,
    Ollama,
    Cohere,
}

impl NativeProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiChatCompletions => "openai_chat_completions",
            Self::OpenaiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::GoogleGemini => "google_gemini",
            Self::AwsBedrock => "aws_bedrock",
            Self::GoogleVertexAi => "google_vertex_ai",
            Self::AzureOpenai => "azure_openai",
            Self::Ollama => "ollama",
            Self::Cohere => "cohere",
        }
    }

    pub fn parse_str(raw: &str) -> Option<Self> {
        match raw {
            "openai_chat_completions" => Some(Self::OpenaiChatCompletions),
            "openai_responses" => Some(Self::OpenaiResponses),
            "anthropic_messages" => Some(Self::AnthropicMessages),
            "google_gemini" => Some(Self::GoogleGemini),
            "aws_bedrock" => Some(Self::AwsBedrock),
            "google_vertex_ai" => Some(Self::GoogleVertexAi),
            "azure_openai" => Some(Self::AzureOpenai),
            "ollama" => Some(Self::Ollama),
            "cohere" => Some(Self::Cohere),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Discovered,
    Compatible,
    MockTested,
    LiveVerified,
    FullSupport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthConfig {
    #[serde(rename = "type")]
    pub auth_type: String,
    #[serde(default)]
    pub header_name: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub query_param: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelOverride {
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub protocols: Option<Vec<String>>,
    #[serde(default)]
    pub default_headers: HashMap<String, String>,
    #[serde(default)]
    pub extra_params: Map<String, Value>,
    #[serde(default)]
    pub thinking: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub protocols: Vec<String>,
    #[serde(default)]
    pub official_endpoints: HashMap<String, String>,
    pub auth: AuthConfig,
    #[serde(default)]
    pub default_headers: HashMap<String, String>,
    #[serde(default)]
    pub extra_params: Map<String, Value>,
    #[serde(default)]
    pub thinking: Option<Value>,
    #[serde(default)]
    pub model_overrides: HashMap<String, ModelOverride>,
    pub verification_status: VerificationStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRegistryDoc {
    pub schema_version: String,
    pub generated_at: String,
    pub providers: HashMap<String, ProviderEntry>,
}

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderEntry>,
}

impl ProviderRegistry {
    pub fn from_json(bytes: &[u8]) -> Result<Self, NativeError> {
        let doc: ProviderRegistryDoc =
            serde_json::from_slice(bytes).map_err(|e| NativeError::Registry(e.to_string()))?;
        if doc.schema_version != "1.0" {
            return Err(NativeError::Registry(format!(
                "unsupported registry schemaVersion: {}",
                doc.schema_version
            )));
        }
        Ok(Self {
            providers: doc.providers,
        })
    }

    pub fn get(&self, provider_id: &str) -> Option<&ProviderEntry> {
        self.providers.get(provider_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn resolve_endpoint(
        &self,
        provider_id: &str,
        protocol: NativeProtocol,
        model_id: Option<&str>,
        endpoint_override: Option<&str>,
        allow_insecure_http: bool,
    ) -> Result<(ProviderEntry, String), NativeError> {
        let entry = self
            .get(provider_id)
            .ok_or_else(|| NativeError::Registry(format!("unknown provider '{provider_id}'")))?
            .clone();

        let proto_name = protocol.as_str();
        if !entry.protocols.iter().any(|p| p == proto_name) {
            return Err(NativeError::UnsupportedProtocol {
                provider: entry.id.clone(),
                protocol: proto_name.to_string(),
            });
        }

        let model_endpoint = model_id
            .and_then(|mid| entry.model_overrides.get(mid))
            .and_then(|ov| ov.endpoint.as_deref());

        let base = endpoint_override
            .or(model_endpoint)
            .or_else(|| entry.official_endpoints.get(proto_name).map(String::as_str))
            .ok_or_else(|| {
                NativeError::InvalidEndpoint(format!(
                    "no endpoint configured for {}/{}",
                    entry.id, proto_name
                ))
            })?;

        let resolved_base = base.trim_end_matches('/').to_string();
        validate_endpoint_url(&resolved_base, allow_insecure_http)?;
        Ok((entry, resolved_base))
    }
}

pub fn validate_endpoint_url(url: &str, allow_insecure_http: bool) -> Result<(), NativeError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| NativeError::InvalidEndpoint(format!("invalid URL '{url}': {e}")))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed.host_str().unwrap_or("");
            let is_loopback = matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]");
            if is_loopback || allow_insecure_http {
                Ok(())
            } else {
                Err(NativeError::InvalidEndpoint(format!(
                    "non-HTTPS endpoint '{url}' is disallowed for non-loopback hosts"
                )))
            }
        }
        other => Err(NativeError::InvalidEndpoint(format!(
            "unsupported URL scheme '{other}'"
        ))),
    }
}
