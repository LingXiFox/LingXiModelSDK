use serde_json::{Map, Value};

use crate::catalog::LingXiModelCatalog;
use crate::errors::ModelCatalogError;
use crate::ordering::{sort_models, sort_providers};
use crate::types::{
    CatalogModel, CatalogModelStatus, CatalogProvider, CatalogRevision, ModelCapabilities,
    ModelLimits, ModelPricing,
};

fn non_empty_str(val: Option<&Value>) -> Option<String> {
    let s = val.and_then(|v| v.as_str())?;
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn opt_str(val: Option<&Value>) -> Option<String> {
    val.and_then(|v| v.as_str()).map(ToString::to_string)
}

fn opt_i64(val: Option<&Value>) -> Option<i64> {
    let v = val?;
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(u) = v.as_u64() {
        return i64::try_from(u).ok();
    }
    None
}

fn opt_f64(val: Option<&Value>) -> Option<f64> {
    let v = val?;
    if v.is_boolean() {
        return None;
    }
    v.as_f64()
}

fn opt_bool(val: Option<&Value>) -> Option<bool> {
    val.and_then(|v| v.as_bool())
}

fn str_vec(val: Option<&Value>) -> Vec<String> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn has_prefix(list: &[String], prefix: &str) -> bool {
    list.iter()
        .any(|item| item.to_lowercase().starts_with(prefix))
}

fn decode_model(provider_id: &str, key: &str, fields: &Map<String, Value>) -> Option<CatalogModel> {
    let id = if !key.trim().is_empty() {
        key.to_string()
    } else {
        non_empty_str(fields.get("id"))?
    };

    let limit_obj = fields.get("limit").and_then(|v| v.as_object());
    let cost_obj = fields.get("cost").and_then(|v| v.as_object());
    let mod_obj = fields.get("modalities").and_then(|v| v.as_object());

    let in_mods = str_vec(mod_obj.and_then(|m| m.get("input")));
    let out_mods = str_vec(mod_obj.and_then(|m| m.get("output")));
    let attachments = fields.get("attachment").and_then(|v| v.as_bool()) == Some(true);
    let vision = attachments || has_prefix(&in_mods, "image");

    let mut efforts = Vec::new();
    if let Some(opts) = fields.get("reasoning_options").and_then(|v| v.as_array()) {
        for opt in opts {
            if let Some(obj) = opt.as_object() {
                efforts.extend(str_vec(obj.get("values")));
            }
        }
    }

    Some(CatalogModel {
        id: id.clone(),
        provider_id: provider_id.to_string(),
        name: opt_str(fields.get("name")).unwrap_or(id),
        family: non_empty_str(fields.get("family")),
        overview: non_empty_str(fields.get("description")),
        release_date: non_empty_str(fields.get("release_date")),
        last_updated: non_empty_str(fields.get("last_updated")),
        knowledge_cutoff: non_empty_str(fields.get("knowledge")),
        canonical_model_id: non_empty_str(fields.get("canonical_model_id")),
        status: CatalogModelStatus::from_raw(fields.get("status").and_then(|v| v.as_str())),
        limits: ModelLimits {
            context: opt_i64(limit_obj.and_then(|l| l.get("context"))),
            input: opt_i64(limit_obj.and_then(|l| l.get("input"))),
            output: opt_i64(limit_obj.and_then(|l| l.get("output"))),
        },
        capabilities: ModelCapabilities {
            reasoning: fields.get("reasoning").and_then(|v| v.as_bool()) == Some(true),
            reasoning_efforts: efforts,
            tool_calling: fields.get("tool_call").and_then(|v| v.as_bool()) == Some(true),
            structured_output: fields.get("structured_output").and_then(|v| v.as_bool())
                == Some(true),
            attachments,
            vision,
            audio_input: has_prefix(&in_mods, "audio"),
            audio_output: has_prefix(&out_mods, "audio"),
            input_modalities: in_mods,
            output_modalities: out_mods,
            open_weights: fields.get("open_weights").and_then(|v| v.as_bool()) == Some(true),
            supports_temperature: opt_bool(fields.get("temperature")),
        },
        pricing: ModelPricing {
            input: opt_f64(cost_obj.and_then(|c| c.get("input"))),
            output: opt_f64(cost_obj.and_then(|c| c.get("output"))),
            cache_read: opt_f64(cost_obj.and_then(|c| c.get("cache_read"))),
            cache_write: opt_f64(cost_obj.and_then(|c| c.get("cache_write"))),
            reasoning: opt_f64(cost_obj.and_then(|c| c.get("reasoning"))),
            input_audio: opt_f64(cost_obj.and_then(|c| c.get("input_audio"))),
            output_audio: opt_f64(cost_obj.and_then(|c| c.get("output_audio"))),
        },
        fields: fields.clone(),
    })
}

fn decode_provider(key: &str, fields: &Map<String, Value>) -> Option<CatalogProvider> {
    let mut models = Vec::new();
    if let Some(models_obj) = fields.get("models").and_then(|v| v.as_object()) {
        for (mid, raw_model) in models_obj {
            let Some(mf) = raw_model.as_object() else {
                continue;
            };
            if let Some(m) = decode_model(key, mid, mf) {
                models.push(m);
            }
        }
    }
    if models.is_empty() && !fields.contains_key("id") {
        return None;
    }
    sort_models(&mut models);

    Some(CatalogProvider {
        id: opt_str(fields.get("id")).unwrap_or_else(|| key.to_string()),
        name: opt_str(fields.get("name")).unwrap_or_else(|| key.to_string()),
        base_url: non_empty_str(fields.get("baseURL")),
        documentation_url: non_empty_str(fields.get("doc")),
        environment_variable_names: str_vec(fields.get("env")),
        models,
        fields: fields.clone(),
    })
}

pub fn decode_catalog(bytes: &[u8]) -> Result<LingXiModelCatalog, ModelCatalogError> {
    let root_val: Value = serde_json::from_slice(bytes)
        .map_err(|e| ModelCatalogError::InvalidCatalog(format!("invalid JSON: {e}")))?;
    let root = root_val
        .as_object()
        .ok_or_else(|| ModelCatalogError::InvalidCatalog("root is not a JSON object".into()))?;

    let providers_obj = root
        .get("providers")
        .and_then(|v| v.as_object())
        .filter(|m| !m.is_empty())
        .ok_or_else(|| ModelCatalogError::InvalidCatalog("providers missing or empty".into()))?;

    let schema_version = opt_str(root.get("schemaVersion"))
        .or_else(|| opt_str(root.get("version")))
        .unwrap_or_else(|| "1.0".to_string());
    let major = schema_version.split('.').next().unwrap_or("");
    if major != "1" && major != "2" {
        return Err(ModelCatalogError::UnsupportedSchemaVersion(schema_version));
    }

    let mut providers = Vec::with_capacity(providers_obj.len());
    for (pid, raw_prov) in providers_obj {
        let Some(pf) = raw_prov.as_object() else {
            continue;
        };
        if let Some(p) = decode_provider(pid, pf) {
            providers.push(p);
        }
    }
    if providers.is_empty() {
        return Err(ModelCatalogError::InvalidCatalog(
            "no valid provider entries".into(),
        ));
    }
    sort_providers(&mut providers, &[]);

    let generated_at = opt_str(root.get("generatedAt")).or_else(|| opt_str(root.get("updatedAt")));
    let catalog_revision = opt_str(root.get("catalogRevision"))
        .or_else(|| opt_str(root.get("catalogHash")))
        .or_else(|| generated_at.clone())
        .unwrap_or_default();

    let total_providers = opt_i64(root.get("totalProviders"))
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(providers.len());
    let counted_models: usize = providers.iter().map(|p| p.model_count()).sum();
    let total_models = opt_i64(root.get("totalModels"))
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(counted_models);

    Ok(LingXiModelCatalog {
        revision: CatalogRevision {
            schema_version,
            catalog_revision,
            catalog_hash: opt_str(root.get("catalogHash")),
            source: opt_str(root.get("source")),
            source_url: opt_str(root.get("sourceURL")),
            source_hash: opt_str(root.get("sourceHash")),
            generated_at,
            source_fetched_at: opt_str(root.get("sourceFetchedAt")),
            total_providers,
            total_models,
        },
        providers,
        raw_document: Some(bytes.to_vec()),
    })
}
