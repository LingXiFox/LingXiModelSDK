import json
import unittest
from pathlib import Path

from lingxi_model_sdk import (
    InvalidCatalogError,
    LingXiModelCatalog,
    ModelFilter,
    UnsupportedSchemaVersionError,
)

CONFORMANCE_DIR = Path(__file__).resolve().parents[3] / "conformance" / "catalog"


class ConformanceTests(unittest.TestCase):
    def test_v2_catalog_conformance(self) -> None:
        exp = json.loads((CONFORMANCE_DIR / "expectations.json").read_text(encoding="utf-8"))
        raw = (CONFORMANCE_DIR / "fixtures" / "v2_catalog.json").read_bytes()
        catalog = LingXiModelCatalog.decoded(raw)

        self.assertEqual(catalog.revision.schema_version, exp["schemaVersion"])
        self.assertEqual(catalog.revision.catalog_revision, exp["catalogRevision"])
        self.assertEqual(catalog.revision.total_providers, exp["totalProviders"])
        self.assertEqual(catalog.total_models, exp["totalModels"])

        self.assertEqual([p.id for p in catalog.sorted_providers()], exp["providerOrder"])
        feat = exp["featuredProviderOrder"]
        self.assertEqual([p.id for p in catalog.sorted_providers(feat["featured"])], feat["expected"])

        self.assertEqual([m.id for m in catalog.models_for_provider("prov10")], exp["prov10ModelOrder"])
        self.assertEqual([m.id for m in catalog.models_for_provider("deepseek")], exp["deepseekModelOrder"])
        self.assertEqual([m.qualified_id for m in catalog.all_models], exp["allModelQualifiedOrder"])

        gpt10 = catalog.model("PROV10", "GPT-10")
        assert gpt10 is not None
        self.assertEqual(gpt10.pricing.input, 0.0)
        self.assertFalse(gpt10.pricing.is_unknown)
        self.assertIs(gpt10.capabilities.supports_temperature, False)
        self.assertTrue(gpt10.capabilities.vision)
        self.assertTrue(gpt10.capabilities.audio_input)
        self.assertTrue(gpt10.capabilities.audio_output)
        self.assertEqual(gpt10.capabilities.reasoning_efforts, ("low", "medium", "high"))
        self.assertTrue(gpt10.fields["experimental"]["warmup"])

        gpt5 = catalog.model("prov10/gpt-5")
        assert gpt5 is not None
        self.assertEqual(gpt5.context_window, 0)
        self.assertIsNone(gpt5.pricing.input)
        self.assertTrue(gpt5.pricing.is_unknown)
        self.assertIsNone(gpt5.capabilities.supports_temperature)
        self.assertFalse(gpt5.capabilities.structured_output)

        self.assertEqual([m.qualified_id for m in catalog.search("gpt", limit=1)], exp["search"]["gptLimit1"])
        self.assertEqual([m.qualified_id for m in catalog.search("deepseek")], exp["search"]["deepseek"])

        self.assertEqual([m.qualified_id for m in catalog.models_matching(ModelFilter(vision=True))], exp["filters"]["vision"])
        self.assertEqual(
            [m.qualified_id for m in catalog.models_matching(ModelFilter(minimum_context_window=200_000))],
            exp["filters"]["minContext200k"],
        )
        self.assertEqual(
            [m.qualified_id for m in catalog.models_matching(ModelFilter(provider_id="deepseek"))],
            exp["filters"]["deepseekDefault"],
        )
        self.assertEqual(
            [m.qualified_id for m in catalog.models_matching(ModelFilter(provider_id="deepseek", include_deprecated=True))],
            exp["filters"]["deepseekIncludeDeprecated"],
        )

    def test_error_and_legacy_fixtures(self) -> None:
        legacy = LingXiModelCatalog.decoded((CONFORMANCE_DIR / "fixtures" / "v1_legacy.json").read_bytes())
        self.assertEqual(legacy.revision.schema_version, "1.0")
        self.assertEqual(legacy.model("ollama", "llama3").context_window, 8192)

        malformed = LingXiModelCatalog.decoded((CONFORMANCE_DIR / "fixtures" / "malformed_entry.json").read_bytes())
        self.assertIsNotNone(malformed.model("p", "published-key"))
        self.assertIsNone(malformed.model("p", "internal-id"))

        with self.assertRaises(UnsupportedSchemaVersionError):
            LingXiModelCatalog.decoded((CONFORMANCE_DIR / "fixtures" / "unsupported_schema.json").read_bytes())

        with self.assertRaises(InvalidCatalogError):
            LingXiModelCatalog.decoded((CONFORMANCE_DIR / "fixtures" / "broken_empty_providers.json").read_bytes())


if __name__ == "__main__":
    unittest.main()
