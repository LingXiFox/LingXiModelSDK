import asyncio
import tempfile
import unittest
from pathlib import Path

from lingxi_model_sdk import (
    CatalogHTTPResponse,
    ModelCatalogConfiguration,
    ModelCatalogLoader,
    TransportError,
    UnexpectedStatusError,
)

FIXTURE = (
    Path(__file__).resolve().parents[3] / "conformance" / "catalog" / "fixtures" / "v2_catalog.json"
).read_bytes()


class StubTransport:
    def __init__(self) -> None:
        self.mode = 0
        self.calls = 0

    def fetch(self, url: str, headers: dict[str, str], timeout: float) -> CatalogHTTPResponse:
        self.calls += 1
        if self.mode == 0:
            if headers.get("If-None-Match") == '"rev-1"':
                return CatalogHTTPResponse(status_code=304, body=b"", etag='"rev-1"')
            return CatalogHTTPResponse(status_code=200, body=FIXTURE, etag='"rev-1"')
        if self.mode == 1:
            return CatalogHTTPResponse(status_code=503, body=b"")
        raise TransportError("offline")


class LoaderTests(unittest.TestCase):
    def test_fresh_etag_offline_and_disk_roundtrip(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            transport = StubTransport()
            cfg = ModelCatalogConfiguration(
                endpoint="https://example.test/models.json",
                max_age_seconds=1800,
                cache_directory=Path(tmp),
            )
            loader = ModelCatalogLoader(cfg, transport)
            cat1 = loader.load()
            self.assertEqual(cat1.revision.catalog_revision, "conf-rev-001")
            loader.load()
            self.assertEqual(transport.calls, 1)

            # Force refresh triggers If-None-Match -> 304
            cat2 = loader.load(force_refresh=True)
            self.assertEqual(cat2.revision.catalog_revision, "conf-rev-001")
            self.assertEqual(transport.calls, 2)

            # Cold loader with network down falls back to disk cache
            transport.mode = 2
            cold = ModelCatalogLoader(
                ModelCatalogConfiguration(
                    endpoint="https://example.test/models.json",
                    max_age_seconds=0,
                    cache_directory=Path(tmp),
                ),
                transport,
            )
            fallback = asyncio.run(cold.load_async())
            self.assertEqual(fallback.total_models, 5)
            self.assertEqual((Path(tmp) / "models.json").read_bytes(), FIXTURE)

        # No cache + 503 raises UnexpectedStatusError
        transport.mode = 1
        empty = ModelCatalogLoader(
            ModelCatalogConfiguration(endpoint="https://example.test/models.json", max_age_seconds=0),
            transport,
        )
        with self.assertRaises(UnexpectedStatusError) as ctx:
            empty.load()
        self.assertEqual(ctx.exception.status_code, 503)


if __name__ == "__main__":
    unittest.main()
