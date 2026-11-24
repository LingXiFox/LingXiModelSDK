import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  FileSystemCatalogStorage,
  ModelCatalogError,
  ModelCatalogLoader,
  type CatalogTransport,
} from "../src/index.ts";

const fixturePath = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../../conformance/catalog/fixtures/v2_catalog.json"
);
const FIXTURE = new Uint8Array(fs.readFileSync(fixturePath));

class StubTransport implements CatalogTransport {
  mode = 0;
  calls = 0;

  async fetchCatalog(_url: string, headers: Record<string, string>) {
    this.calls += 1;
    if (this.mode === 0) {
      if (headers["If-None-Match"] === '"rev-1"') {
        return { status: 304, body: new Uint8Array(), etag: '"rev-1"' };
      }
      return { status: 200, body: FIXTURE, etag: '"rev-1"' };
    }
    if (this.mode === 1) {
      return { status: 503, body: new Uint8Array(), etag: null };
    }
    throw new ModelCatalogError("transport", "offline");
  }
}

test("loader handles fresh cache, ETag 304, offline disk fallback, and 503 failure", async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "lingxi-ts-cache-"));
  try {
    const transport = new StubTransport();
    const loader = new ModelCatalogLoader(
      {
        endpoint: "https://example.test/models.json",
        maxAgeMs: 1_800_000,
        storage: new FileSystemCatalogStorage(dir),
      },
      transport
    );

    const cat1 = await loader.load();
    assert.equal(cat1.revision.catalogRevision, "conf-rev-001");
    await loader.load();
    assert.equal(transport.calls, 1);

    const cat2 = await loader.load(true);
    assert.equal(cat2.revision.catalogRevision, "conf-rev-001");
    assert.equal(transport.calls, 2);

    transport.mode = 2;
    const cold = new ModelCatalogLoader(
      {
        endpoint: "https://example.test/models.json",
        maxAgeMs: 0,
        storage: new FileSystemCatalogStorage(dir),
      },
      transport
    );
    const fallback = await cold.load();
    assert.equal(fallback.totalModels, 5);
    assert.deepEqual(new Uint8Array(fs.readFileSync(path.join(dir, "models.json"))), FIXTURE);

    transport.mode = 1;
    const empty = new ModelCatalogLoader(
      { endpoint: "https://example.test/models.json", maxAgeMs: 0 },
      transport
    );
    await assert.rejects(
      () => empty.load(),
      (err: unknown) =>
        err instanceof ModelCatalogError &&
        err.code === "unexpected_status" &&
        err.statusCode === 503
    );
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
