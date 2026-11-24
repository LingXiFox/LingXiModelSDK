use std::path::PathBuf;

use lingxi_model_sdk::{LingXiModelCatalog, ModelCatalogError, ModelFilter};
use serde_json::Value;

fn conformance_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("conformance/catalog")
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(conformance_dir().join("fixtures").join(name)).unwrap()
}

fn str_array(val: &Value) -> Vec<String> {
    val.as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn conformance_v2_catalog() {
    let exp_bytes = std::fs::read(conformance_dir().join("expectations.json")).unwrap();
    let exp: Value = serde_json::from_slice(&exp_bytes).unwrap();
    let catalog = LingXiModelCatalog::decode(&read_fixture("v2_catalog.json")).unwrap();

    assert_eq!(
        catalog.revision.schema_version,
        exp["schemaVersion"].as_str().unwrap()
    );
    assert_eq!(
        catalog.revision.catalog_revision,
        exp["catalogRevision"].as_str().unwrap()
    );
    assert_eq!(
        catalog.revision.total_providers,
        exp["totalProviders"].as_u64().unwrap() as usize
    );
    assert_eq!(
        catalog.total_models(),
        exp["totalModels"].as_u64().unwrap() as usize
    );

    let prov_ids: Vec<String> = catalog
        .sorted_providers(&[])
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(prov_ids, str_array(&exp["providerOrder"]));

    let feat: Vec<String> = str_array(&exp["featuredProviderOrder"]["featured"]);
    let feat_refs: Vec<&str> = feat.iter().map(String::as_str).collect();
    let feat_ids: Vec<String> = catalog
        .sorted_providers(&feat_refs)
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(
        feat_ids,
        str_array(&exp["featuredProviderOrder"]["expected"])
    );

    let prov10_ids: Vec<String> = catalog
        .models_for_provider("prov10")
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(prov10_ids, str_array(&exp["prov10ModelOrder"]));

    let ds_ids: Vec<String> = catalog
        .models_for_provider("deepseek")
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ds_ids, str_array(&exp["deepseekModelOrder"]));

    let all_ids: Vec<String> = catalog
        .all_models()
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(all_ids, str_array(&exp["allModelQualifiedOrder"]));

    let gpt10 = catalog.model_in_provider("PROV10", "GPT-10").unwrap();
    assert_eq!(gpt10.pricing.input, Some(0.0));
    assert!(!gpt10.pricing.is_unknown());
    assert_eq!(gpt10.capabilities.supports_temperature, Some(false));
    assert!(gpt10.capabilities.vision);
    assert!(gpt10.capabilities.audio_input);
    assert!(gpt10.capabilities.audio_output);
    assert_eq!(
        gpt10.capabilities.reasoning_efforts,
        vec!["low", "medium", "high"]
    );
    assert_eq!(
        gpt10.fields["experimental"]["warmup"].as_bool(),
        Some(true)
    );

    let gpt5 = catalog.model("prov10/gpt-5").unwrap();
    assert_eq!(gpt5.context_window(), Some(0));
    assert!(gpt5.pricing.is_unknown());
    assert_eq!(gpt5.capabilities.supports_temperature, None);
    assert!(!gpt5.capabilities.structured_output);

    let search_gpt: Vec<String> = catalog
        .search("gpt", Some(1))
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(search_gpt, str_array(&exp["search"]["gptLimit1"]));

    let search_ds: Vec<String> = catalog
        .search("deepseek", None)
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(search_ds, str_array(&exp["search"]["deepseek"]));

    let vision_ids: Vec<String> = catalog
        .models_matching(&ModelFilter {
            vision: Some(true),
            ..Default::default()
        })
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(vision_ids, str_array(&exp["filters"]["vision"]));

    let ctx_ids: Vec<String> = catalog
        .models_matching(&ModelFilter {
            minimum_context_window: Some(200_000),
            ..Default::default()
        })
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(ctx_ids, str_array(&exp["filters"]["minContext200k"]));

    let ds_default: Vec<String> = catalog
        .models_matching(&ModelFilter {
            provider_id: Some("deepseek".into()),
            ..Default::default()
        })
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(ds_default, str_array(&exp["filters"]["deepseekDefault"]));

    let ds_all: Vec<String> = catalog
        .models_matching(&ModelFilter {
            provider_id: Some("deepseek".into()),
            include_deprecated: true,
            ..Default::default()
        })
        .into_iter()
        .map(|m| m.qualified_id())
        .collect();
    assert_eq!(
        ds_all,
        str_array(&exp["filters"]["deepseekIncludeDeprecated"])
    );
}

#[test]
fn conformance_error_and_legacy_fixtures() {
    let legacy = LingXiModelCatalog::decode(&read_fixture("v1_legacy.json")).unwrap();
    assert_eq!(legacy.revision.schema_version, "1.0");
    assert_eq!(
        legacy
            .model_in_provider("ollama", "llama3")
            .unwrap()
            .context_window(),
        Some(8192)
    );

    let malformed = LingXiModelCatalog::decode(&read_fixture("malformed_entry.json")).unwrap();
    assert!(malformed.model_in_provider("p", "published-key").is_some());
    assert!(malformed.model_in_provider("p", "internal-id").is_none());

    assert_eq!(
        LingXiModelCatalog::decode(&read_fixture("unsupported_schema.json")),
        Err(ModelCatalogError::UnsupportedSchemaVersion("3.0".into()))
    );
    assert!(matches!(
        LingXiModelCatalog::decode(&read_fixture("broken_empty_providers.json")),
        Err(ModelCatalogError::InvalidCatalog(_))
    ));
}
