import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ModelCatalogError, decodeCatalog, isPricingUnknown } from "../src/index.ts";

const conformanceDir = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../../conformance/catalog"
);

function readFixture(name: string): Uint8Array {
  return fs.readFileSync(path.join(conformanceDir, "fixtures", name));
}

test("conformance v2 catalog decoding, ordering, lookup, filter and three-state semantics", () => {
  const exp = JSON.parse(fs.readFileSync(path.join(conformanceDir, "expectations.json"), "utf8"));
  const catalog = decodeCatalog(readFixture("v2_catalog.json"));

  assert.equal(catalog.revision.schemaVersion, exp.schemaVersion);
  assert.equal(catalog.revision.catalogRevision, exp.catalogRevision);
  assert.equal(catalog.revision.totalProviders, exp.totalProviders);
  assert.equal(catalog.totalModels, exp.totalModels);

  assert.deepEqual(
    catalog.sortedProviders().map((p) => p.id),
    exp.providerOrder
  );
  assert.deepEqual(
    catalog.sortedProviders(exp.featuredProviderOrder.featured).map((p) => p.id),
    exp.featuredProviderOrder.expected
  );

  assert.deepEqual(
    catalog.modelsForProvider("prov10").map((m) => m.id),
    exp.prov10ModelOrder
  );
  assert.deepEqual(
    catalog.modelsForProvider("deepseek").map((m) => m.id),
    exp.deepseekModelOrder
  );
  assert.deepEqual(
    catalog.allModels.map((m) => m.qualifiedID),
    exp.allModelQualifiedOrder
  );

  const gpt10 = catalog.model("PROV10", "GPT-10");
  assert.ok(gpt10);
  assert.equal(gpt10.pricing.input, 0);
  assert.equal(isPricingUnknown(gpt10.pricing), false);
  assert.equal(gpt10.capabilities.supportsTemperature, false);
  assert.equal(gpt10.capabilities.vision, true);
  assert.equal(gpt10.capabilities.audioInput, true);
  assert.equal(gpt10.capabilities.audioOutput, true);
  assert.deepEqual(gpt10.capabilities.reasoningEfforts, ["low", "medium", "high"]);
  assert.equal((gpt10.fields.experimental as { warmup?: boolean }).warmup, true);

  const gpt5 = catalog.model("prov10/gpt-5");
  assert.ok(gpt5);
  assert.equal(gpt5.limits.context, 0);
  assert.equal(gpt5.pricing.input, null);
  assert.equal(isPricingUnknown(gpt5.pricing), true);
  assert.equal(gpt5.capabilities.supportsTemperature, null);
  assert.equal(gpt5.capabilities.structuredOutput, false);

  assert.deepEqual(
    catalog.search("gpt", 1).map((m) => m.qualifiedID),
    exp.search.gptLimit1
  );
  assert.deepEqual(
    catalog.search("deepseek").map((m) => m.qualifiedID),
    exp.search.deepseek
  );

  assert.deepEqual(
    catalog.modelsMatching({ vision: true }).map((m) => m.qualifiedID),
    exp.filters.vision
  );
  assert.deepEqual(
    catalog.modelsMatching({ minimumContextWindow: 200_000 }).map((m) => m.qualifiedID),
    exp.filters.minContext200k
  );
  assert.deepEqual(
    catalog.modelsMatching({ providerID: "deepseek" }).map((m) => m.qualifiedID),
    exp.filters.deepseekDefault
  );
  assert.deepEqual(
    catalog.modelsMatching({ providerID: "deepseek", includeDeprecated: true }).map((m) => m.qualifiedID),
    exp.filters.deepseekIncludeDeprecated
  );
});

test("conformance v1 legacy, malformed, unsupported schema, and broken fixtures", () => {
  const legacy = decodeCatalog(readFixture("v1_legacy.json"));
  assert.equal(legacy.revision.schemaVersion, "1.0");
  assert.equal(legacy.model("ollama", "llama3")?.limits.context, 8192);

  const malformed = decodeCatalog(readFixture("malformed_entry.json"));
  assert.ok(malformed.model("p", "published-key"));
  assert.equal(malformed.model("p", "internal-id"), undefined);

  assert.throws(
    () => decodeCatalog(readFixture("unsupported_schema.json")),
    (err: unknown) => err instanceof ModelCatalogError && err.code === "unsupported_schema_version"
  );
  assert.throws(
    () => decodeCatalog(readFixture("broken_empty_providers.json")),
    (err: unknown) => err instanceof ModelCatalogError && err.code === "invalid_catalog"
  );
});
