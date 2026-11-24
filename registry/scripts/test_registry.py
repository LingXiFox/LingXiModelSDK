import json
import unittest
from pathlib import Path

from classify_catalog import build_registry

ROOT = Path(__file__).resolve().parents[1]


class RegistryTests(unittest.TestCase):
    def test_committed_registry_invariants(self) -> None:
        reg_path = ROOT / "registry.json"
        self.assertTrue(reg_path.exists(), "registry/registry.json missing")
        reg = json.loads(reg_path.read_text(encoding="utf-8"))
        self.assertEqual(reg["schemaVersion"], "1.0")
        providers = reg["providers"]
        self.assertGreaterEqual(len(providers), 300)

        summary = reg["summary"]
        self.assertEqual(summary["totalProviders"], len(providers))
        self.assertEqual(summary["live_verified"], 0)
        self.assertEqual(summary["full_support"], 0)

        for pid, entry in providers.items():
            self.assertEqual(entry["id"], pid)
            status = entry["verificationStatus"]
            if status == "discovered":
                self.assertEqual(entry["protocols"], [], f"discovered provider {pid} must not infer protocols")
            elif status == "mock_tested":
                self.assertTrue(entry["verificationEvidence"], f"mock_tested provider {pid} missing evidence")

    def test_classifier_never_infers_protocols_from_npm(self) -> None:
        tmp_catalog = ROOT / "scripts" / "_tmp_test_catalog.json"
        try:
            tmp_catalog.write_text(
                json.dumps(
                    {
                        "mystery-vendor": {
                            "name": "Mystery",
                            "npm": "@ai-sdk/openai-compatible",
                            "api": "https://mystery.example/v1",
                            "env": ["MYSTERY_KEY"],
                            "models": {"m1": {"name": "M1"}},
                        }
                    }
                ),
                encoding="utf-8",
            )
            reg = build_registry(ROOT / "curated", tmp_catalog, generated_at="2026-11-24T00:00:00Z")
            entry = reg["providers"]["mystery-vendor"]
            self.assertEqual(entry["verificationStatus"], "discovered")
            self.assertEqual(entry["protocols"], [])
            self.assertEqual(entry["discoveryHints"]["upstreamNpm"], "@ai-sdk/openai-compatible")
        finally:
            if tmp_catalog.exists():
                tmp_catalog.unlink()


if __name__ == "__main__":
    unittest.main()
