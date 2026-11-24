use crate::decoder::decode_catalog;
use crate::errors::ModelCatalogError;
use crate::ordering::{sort_models, sort_providers};
use crate::types::{CatalogModel, CatalogProvider, CatalogRevision};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelFilter {
    pub provider_id: Option<String>,
    pub query: Option<String>,
    pub reasoning: Option<bool>,
    pub tool_calling: Option<bool>,
    pub structured_output: Option<bool>,
    pub vision: Option<bool>,
    pub attachments: Option<bool>,
    pub open_weights: Option<bool>,
    pub minimum_context_window: Option<i64>,
    pub minimum_max_output_tokens: Option<i64>,
    pub include_deprecated: bool,
    pub featured_provider_ids: Vec<String>,
}

fn norm(text: &str) -> String {
    text.trim().to_lowercase()
}

#[derive(Debug, Clone, PartialEq)]
pub struct LingXiModelCatalog {
    pub revision: CatalogRevision,
    pub providers: Vec<CatalogProvider>,
    pub raw_document: Option<Vec<u8>>,
}

impl LingXiModelCatalog {
    pub fn decode(bytes: &[u8]) -> Result<Self, ModelCatalogError> {
        decode_catalog(bytes)
    }

    pub fn total_models(&self) -> usize {
        self.providers.iter().map(|p| p.model_count()).sum()
    }

    pub fn provider(&self, provider_id: &str) -> Option<&CatalogProvider> {
        let key = norm(provider_id);
        self.providers.iter().find(|p| norm(&p.id) == key)
    }

    pub fn model_in_provider(&self, provider_id: &str, model_id: &str) -> Option<&CatalogModel> {
        let prov = self.provider(provider_id)?;
        let key = norm(model_id);
        prov.models.iter().find(|m| norm(&m.id) == key)
    }

    pub fn model(&self, id: &str) -> Option<&CatalogModel> {
        if let Some((pid, mid)) = id.split_once('/') {
            return self.model_in_provider(pid, mid);
        }
        let key = norm(id);
        for prov in &self.providers {
            if let Some(m) = prov.models.iter().find(|m| norm(&m.id) == key) {
                return Some(m);
            }
        }
        None
    }

    pub fn sorted_providers(&self, featured: &[&str]) -> Vec<CatalogProvider> {
        let mut out = self.providers.clone();
        sort_providers(&mut out, featured);
        out
    }

    pub fn models_for_provider(&self, provider_id: &str) -> Vec<CatalogModel> {
        let Some(prov) = self.provider(provider_id) else {
            return Vec::new();
        };
        let mut out = prov.models.clone();
        sort_models(&mut out);
        out
    }

    pub fn all_models(&self) -> Vec<CatalogModel> {
        let mut out = Vec::with_capacity(self.total_models());
        for prov in self.sorted_providers(&[]) {
            let mut models = prov.models;
            sort_models(&mut models);
            out.extend(models);
        }
        out
    }

    pub fn search(&self, query: &str, limit: Option<usize>) -> Vec<CatalogModel> {
        let needle = norm(query);
        if needle.is_empty() {
            let mut all = self.all_models();
            if let Some(lim) = limit {
                all.truncate(lim);
            }
            return all;
        }
        let mut matches = Vec::new();
        for prov in self.sorted_providers(&[]) {
            let mut ordered = prov.models;
            sort_models(&mut ordered);
            if norm(&prov.id).contains(&needle) || norm(&prov.name).contains(&needle) {
                matches.extend(ordered);
            } else {
                for m in ordered {
                    let hit = [
                        m.id.as_str(),
                        m.name.as_str(),
                        m.family.as_deref().unwrap_or(""),
                        m.overview.as_deref().unwrap_or(""),
                    ]
                    .iter()
                    .any(|s| norm(s).contains(&needle));
                    if hit {
                        matches.push(m);
                    }
                }
            }
            if let Some(lim) = limit {
                if matches.len() >= lim {
                    matches.truncate(lim);
                    return matches;
                }
            }
        }
        if let Some(lim) = limit {
            matches.truncate(lim);
        }
        matches
    }

    pub fn models_matching(&self, filter: &ModelFilter) -> Vec<CatalogModel> {
        let featured_refs: Vec<&str> = filter
            .featured_provider_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut matches = Vec::new();
        for prov in self.sorted_providers(&featured_refs) {
            if let Some(pid) = &filter.provider_id {
                if norm(&prov.id) != norm(pid) {
                    continue;
                }
            }
            let mut ordered = prov.models.clone();
            sort_models(&mut ordered);
            for m in ordered {
                if filter_matches(&m, &prov, filter) {
                    matches.push(m);
                }
            }
        }
        matches
    }
}

fn filter_matches(m: &CatalogModel, p: &CatalogProvider, f: &ModelFilter) -> bool {
    if !f.include_deprecated && !m.status.is_selectable() {
        return false;
    }
    if let Some(v) = f.reasoning {
        if m.capabilities.reasoning != v {
            return false;
        }
    }
    if let Some(v) = f.tool_calling {
        if m.capabilities.tool_calling != v {
            return false;
        }
    }
    if let Some(v) = f.structured_output {
        if m.capabilities.structured_output != v {
            return false;
        }
    }
    if let Some(v) = f.vision {
        if m.capabilities.vision != v {
            return false;
        }
    }
    if let Some(v) = f.attachments {
        if m.capabilities.attachments != v {
            return false;
        }
    }
    if let Some(v) = f.open_weights {
        if m.capabilities.open_weights != v {
            return false;
        }
    }
    if let Some(min_ctx) = f.minimum_context_window {
        if m.limits.context.unwrap_or(0) < min_ctx {
            return false;
        }
    }
    if let Some(min_out) = f.minimum_max_output_tokens {
        if m.limits.output.unwrap_or(0) < min_out {
            return false;
        }
    }
    if let Some(q) = &f.query {
        let qn = norm(q);
        if !qn.is_empty() {
            let hit = [
                m.id.as_str(),
                m.name.as_str(),
                m.family.as_deref().unwrap_or(""),
                m.overview.as_deref().unwrap_or(""),
                p.id.as_str(),
                p.name.as_str(),
            ]
            .iter()
            .any(|s| norm(s).contains(&qn));
            if !hit {
                return false;
            }
        }
    }
    true
}
