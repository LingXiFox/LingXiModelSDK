use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lingxi_model_sdk::{
    BoxFuture, CatalogHttpResponse, CatalogTransport, ModelCatalogConfiguration, ModelCatalogError,
    ModelCatalogLoader,
};

fn fixture_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance/catalog/fixtures/v2_catalog.json");
    std::fs::read(path).unwrap()
}

struct StubTransport {
    mode: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
    body: Vec<u8>,
}

impl CatalogTransport for StubTransport {
    fn fetch<'a>(
        &'a self,
        _url: &'a str,
        headers: HashMap<String, String>,
        _timeout: Duration,
    ) -> BoxFuture<'a, Result<CatalogHttpResponse, ModelCatalogError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.mode.load(Ordering::SeqCst) {
                0 => {
                    if headers.get("If-None-Match").map(String::as_str) == Some("\"rev-1\"") {
                        Ok(CatalogHttpResponse {
                            status: 304,
                            body: Vec::new(),
                            etag: Some("\"rev-1\"".into()),
                        })
                    } else {
                        Ok(CatalogHttpResponse {
                            status: 200,
                            body: self.body.clone(),
                            etag: Some("\"rev-1\"".into()),
                        })
                    }
                }
                1 => Ok(CatalogHttpResponse {
                    status: 503,
                    body: Vec::new(),
                    etag: None,
                }),
                _ => Err(ModelCatalogError::Transport("offline".into())),
            }
        })
    }
}

#[tokio::test]
async fn loader_fresh_etag_offline_and_disk_roundtrip() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("lingxi-rust-cache-{nanos}"));
    let body = fixture_bytes();

    let mode = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let cfg = ModelCatalogConfiguration {
        endpoint: "https://example.test/models.json".into(),
        max_age: Duration::from_secs(1800),
        cache_directory: Some(dir.clone()),
        ..Default::default()
    };
    let loader = ModelCatalogLoader::with_transport(
        cfg.clone(),
        StubTransport {
            mode: mode.clone(),
            calls: calls.clone(),
            body: body.clone(),
        },
    );

    let cat1 = loader.load(false).await.unwrap();
    assert_eq!(cat1.revision.catalog_revision, "conf-rev-001");
    loader.load(false).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let cat2 = loader.load(true).await.unwrap();
    assert_eq!(cat2.revision.catalog_revision, "conf-rev-001");
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    mode.store(2, Ordering::SeqCst);
    let cold = ModelCatalogLoader::with_transport(
        ModelCatalogConfiguration {
            max_age: Duration::ZERO,
            ..cfg
        },
        StubTransport {
            mode: mode.clone(),
            calls: calls.clone(),
            body: body.clone(),
        },
    );
    let fallback = cold.load(false).await.unwrap();
    assert_eq!(fallback.total_models(), 5);
    assert_eq!(std::fs::read(dir.join("models.json")).unwrap(), body);
    let _ = std::fs::remove_dir_all(&dir);

    mode.store(1, Ordering::SeqCst);
    let empty = ModelCatalogLoader::with_transport(
        ModelCatalogConfiguration {
            endpoint: "https://example.test/models.json".into(),
            max_age: Duration::ZERO,
            cache_directory: None,
            ..Default::default()
        },
        StubTransport {
            mode,
            calls,
            body,
        },
    );
    assert_eq!(
        empty.load(false).await,
        Err(ModelCatalogError::UnexpectedStatus(503))
    );
}
