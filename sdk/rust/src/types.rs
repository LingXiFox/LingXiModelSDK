use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogModelStatus {
    Active,
    Preview,
    Deprecated,
    Retired,
    Unknown,
}

impl CatalogModelStatus {
    pub fn from_raw(raw: Option<&str>) -> Self {
        match raw {
            Some("deprecated") | Some("retired") => Self::Deprecated,
            Some("beta") | Some("preview") => Self::Preview,
            Some("active") => Self::Active,
            _ => Self::Unknown,
        }
    }

    pub fn is_selectable(self) -> bool {
        !matches!(self, Self::Deprecated | Self::Retired)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelLimits {
    pub context: Option<i64>,
    pub input: Option<i64>,
    pub output: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub reasoning: bool,
    pub reasoning_efforts: Vec<String>,
    pub tool_calling: bool,
    pub structured_output: bool,
    pub attachments: bool,
    pub vision: bool,
    pub audio_input: bool,
    pub audio_output: bool,
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub open_weights: bool,
    pub supports_temperature: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelPricing {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub reasoning: Option<f64>,
    pub input_audio: Option<f64>,
    pub output_audio: Option<f64>,
}

impl ModelPricing {
    pub fn is_unknown(&self) -> bool {
        self.input.is_none()
            && self.output.is_none()
            && self.cache_read.is_none()
            && self.cache_write.is_none()
            && self.reasoning.is_none()
            && self.input_audio.is_none()
            && self.output_audio.is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogModel {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub family: Option<String>,
    pub overview: Option<String>,
    pub release_date: Option<String>,
    pub last_updated: Option<String>,
    pub knowledge_cutoff: Option<String>,
    pub canonical_model_id: Option<String>,
    pub status: CatalogModelStatus,
    pub limits: ModelLimits,
    pub capabilities: ModelCapabilities,
    pub pricing: ModelPricing,
    pub fields: Map<String, Value>,
}

impl CatalogModel {
    pub fn qualified_id(&self) -> String {
        format!("{}/{}", self.provider_id, self.id)
    }

    pub fn context_window(&self) -> Option<i64> {
        self.limits.context
    }

    pub fn max_output_tokens(&self) -> Option<i64> {
        self.limits.output
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogProvider {
    pub id: String,
    pub name: String,
    pub base_url: Option<String>,
    pub documentation_url: Option<String>,
    pub environment_variable_names: Vec<String>,
    pub models: Vec<CatalogModel>,
    pub fields: Map<String, Value>,
}

impl CatalogProvider {
    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogRevision {
    pub schema_version: String,
    pub catalog_revision: String,
    pub catalog_hash: Option<String>,
    pub source: Option<String>,
    pub source_url: Option<String>,
    pub source_hash: Option<String>,
    pub generated_at: Option<String>,
    pub source_fetched_at: Option<String>,
    pub total_providers: usize,
    pub total_models: usize,
}
