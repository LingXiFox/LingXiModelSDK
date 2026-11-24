use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::catalog::LingXiModelCatalog;
use crate::errors::ModelCatalogError;

pub const DEFAULT_ENDPOINT: &str = "https://models.lingxifox.cn/models.json";

#[derive(Debug, Clone)]
pub struct ModelCatalogConfiguration {
    pub endpoint: String,
    pub max_age: Duration,
    pub cache_directory: Option<PathBuf>,
    pub request_timeout: Duration,
    pub user_agent: String,
}

impl Default for ModelCatalogConfiguration {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            max_age: Duration::from_secs(1800),
            cache_directory: None,
            request_timeout: Duration::from_secs(30),
            user_agent: "LingXiModelSDK-Rust/2.0".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub etag: Option<String>,
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait CatalogTransport: Send + Sync {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        headers: HashMap<String, String>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<CatalogHttpResponse, ModelCatalogError>>;
}

pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl Default for ReqwestTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl ReqwestTransport {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl CatalogTransport for ReqwestTransport {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
        headers: HashMap<String, String>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<CatalogHttpResponse, ModelCatalogError>> {
        Box::pin(async move {
            let mut req = self.client.get(url).timeout(timeout);
            for (k, v) in headers {
                req = req.header(&k, &v);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| ModelCatalogError::Transport(e.to_string()))?;
            let status = resp.status().as_u16();
            let etag = resp
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let body = resp
                .bytes()
                .await
                .map_err(|e| ModelCatalogError::Transport(e.to_string()))?
                .to_vec();
            Ok(CatalogHttpResponse { status, body, etag })
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    etag: Option<String>,
    fetched_at_epoch_secs: u64,
}

#[derive(Debug, Clone)]
pub struct CachedModelCatalog {
    pub catalog: LingXiModelCatalog,
    pub fetched_at: SystemTime,
    pub etag: Option<String>,
}

impl CachedModelCatalog {
    pub fn is_fresh(&self, max_age: Duration, now: SystemTime) -> bool {
        if max_age.is_zero() {
            return false;
        }
        now.duration_since(self.fetched_at)
            .map(|elapsed| elapsed < max_age)
            .unwrap_or(false)
    }
}

pub struct ModelCatalogLoader<T: CatalogTransport = ReqwestTransport> {
    config: ModelCatalogConfiguration,
    transport: T,
    memory: Mutex<Option<CachedModelCatalog>>,
}

impl ModelCatalogLoader<ReqwestTransport> {
    pub fn new(config: ModelCatalogConfiguration) -> Self {
        Self::with_transport(config, ReqwestTransport::new())
    }
}

impl<T: CatalogTransport> ModelCatalogLoader<T> {
    pub fn with_transport(config: ModelCatalogConfiguration, transport: T) -> Self {
        Self {
            config,
            transport,
            memory: Mutex::new(None),
        }
    }

    pub fn cached(&self) -> Option<CachedModelCatalog> {
        let mut guard = self.memory.lock().unwrap();
        if let Some(mem) = guard.as_ref() {
            return Some(mem.clone());
        }
        let dir = self.config.cache_directory.as_ref()?;
        let raw = std::fs::read(dir.join("models.json")).ok()?;
        let catalog = LingXiModelCatalog::decode(&raw).ok()?;
        let mut etag = None;
        let mut fetched_at = UNIX_EPOCH;
        if let Ok(meta_bytes) = std::fs::read(dir.join("models.json.meta.json")) {
            if let Ok(meta) = serde_json::from_slice::<CacheMeta>(&meta_bytes) {
                etag = meta.etag;
                fetched_at = UNIX_EPOCH + Duration::from_secs(meta.fetched_at_epoch_secs);
            }
        }
        let entry = CachedModelCatalog {
            catalog,
            fetched_at,
            etag,
        };
        *guard = Some(entry.clone());
        Some(entry)
    }

    pub async fn load(&self, force_refresh: bool) -> Result<LingXiModelCatalog, ModelCatalogError> {
        let cached = self.cached();
        let now = SystemTime::now();
        if !force_refresh {
            if let Some(ref c) = cached {
                if c.is_fresh(self.config.max_age, now) {
                    return Ok(c.catalog.clone());
                }
            }
        }

        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("User-Agent".to_string(), self.config.user_agent.clone());
        if let Some(ref c) = cached {
            if let Some(ref etag) = c.etag {
                headers.insert("If-None-Match".to_string(), etag.clone());
            }
        }

        let result = self
            .transport
            .fetch(&self.config.endpoint, headers, self.config.request_timeout)
            .await;

        match result {
            Ok(resp) if resp.status == 304 => {
                if let Some(c) = cached {
                    let etag = resp.etag.or(c.etag);
                    let ts = SystemTime::now();
                    *self.memory.lock().unwrap() = Some(CachedModelCatalog {
                        catalog: c.catalog.clone(),
                        fetched_at: ts,
                        etag: etag.clone(),
                    });
                    self.persist_meta(etag.as_deref(), ts);
                    return Ok(c.catalog);
                }
                Err(ModelCatalogError::UnexpectedStatus(304))
            }
            Ok(resp) if (200..=299).contains(&resp.status) => {
                match LingXiModelCatalog::decode(&resp.body) {
                    Ok(catalog) => {
                        let ts = SystemTime::now();
                        *self.memory.lock().unwrap() = Some(CachedModelCatalog {
                            catalog: catalog.clone(),
                            fetched_at: ts,
                            etag: resp.etag.clone(),
                        });
                        self.persist_catalog(&resp.body, resp.etag.as_deref(), ts);
                        Ok(catalog)
                    }
                    Err(e) => {
                        if let Some(c) = cached {
                            return Ok(c.catalog);
                        }
                        Err(e)
                    }
                }
            }
            Ok(resp) => {
                if let Some(c) = cached {
                    return Ok(c.catalog);
                }
                Err(ModelCatalogError::UnexpectedStatus(resp.status))
            }
            Err(e) => {
                if let Some(c) = cached {
                    return Ok(c.catalog);
                }
                Err(e)
            }
        }
    }

    fn persist_catalog(&self, raw: &[u8], etag: Option<&str>, fetched_at: SystemTime) {
        let Some(ref dir) = self.config.cache_directory else {
            return;
        };
        let _ = write_atomic(&dir.join("models.json"), raw);
        self.persist_meta(etag, fetched_at);
    }

    fn persist_meta(&self, etag: Option<&str>, fetched_at: SystemTime) {
        let Some(ref dir) = self.config.cache_directory else {
            return;
        };
        let secs = fetched_at
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(bytes) = serde_json::to_vec(&CacheMeta {
            etag: etag.map(ToString::to_string),
            fetched_at_epoch_secs: secs,
        }) {
            let _ = write_atomic(&dir.join("models.json.meta.json"), &bytes);
        }
    }
}

fn write_atomic(target: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = target.with_file_name(format!(".cache-{}-{nanos}", std::process::id()));
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, target)
}
