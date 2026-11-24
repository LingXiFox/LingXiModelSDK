import { LingXiModelCatalog } from "./catalog.ts";
import { ModelCatalogError } from "./errors.ts";
import { modelsInStableOrder, sortProviders } from "./ordering.ts";
import {
  parseModelStatus,
  type CatalogModel,
  type CatalogProvider,
  type CatalogRevision,
} from "./types.ts";

const SUPPORTED_MAJORS = new Set(["1", "2"]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function optString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function nonEmpty(value: unknown): string | null {
  if (typeof value !== "string") return null;
  return value.trim().length > 0 ? value : null;
}

function optInt(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) ? value : null;
}

function optNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function optBool(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function stringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function parseTimestamp(value: unknown): Date | null {
  if (typeof value !== "string" || !value) return null;
  const ms = Date.parse(value);
  return Number.isNaN(ms) ? null : new Date(ms);
}

function decodeModel(
  providerID: string,
  key: string,
  fields: Record<string, unknown>
): CatalogModel | null {
  const id = nonEmpty(key) ?? nonEmpty(fields.id);
  if (id === null) return null;

  const limitObj = isRecord(fields.limit) ? fields.limit : {};
  const costObj = isRecord(fields.cost) ? fields.cost : {};
  const modObj = isRecord(fields.modalities) ? fields.modalities : {};

  const inputModalities = stringArray(modObj.input);
  const outputModalities = stringArray(modObj.output);
  const attachments = fields.attachment === true;
  const vision =
    attachments || inputModalities.some((m) => m.toLowerCase().startsWith("image"));

  const reasoningEfforts: string[] = [];
  if (Array.isArray(fields.reasoning_options)) {
    for (const opt of fields.reasoning_options) {
      if (isRecord(opt)) {
        reasoningEfforts.push(...stringArray(opt.values));
      }
    }
  }

  return {
    id,
    providerID,
    qualifiedID: `${providerID}/${id}`,
    name: optString(fields.name) ?? id,
    family: nonEmpty(fields.family),
    overview: nonEmpty(fields.description),
    releaseDate: nonEmpty(fields.release_date),
    lastUpdated: nonEmpty(fields.last_updated),
    knowledgeCutoff: nonEmpty(fields.knowledge),
    canonicalModelID: nonEmpty(fields.canonical_model_id),
    status: parseModelStatus(fields.status),
    limits: {
      context: optInt(limitObj.context),
      input: optInt(limitObj.input),
      output: optInt(limitObj.output),
    },
    capabilities: {
      reasoning: fields.reasoning === true,
      reasoningEfforts,
      toolCalling: fields.tool_call === true,
      structuredOutput: fields.structured_output === true,
      attachments,
      vision,
      audioInput: inputModalities.some((m) => m.toLowerCase().startsWith("audio")),
      audioOutput: outputModalities.some((m) => m.toLowerCase().startsWith("audio")),
      inputModalities,
      outputModalities,
      openWeights: fields.open_weights === true,
      supportsTemperature: optBool(fields.temperature),
    },
    pricing: {
      input: optNumber(costObj.input),
      output: optNumber(costObj.output),
      cacheRead: optNumber(costObj.cache_read),
      cacheWrite: optNumber(costObj.cache_write),
      reasoning: optNumber(costObj.reasoning),
      inputAudio: optNumber(costObj.input_audio),
      outputAudio: optNumber(costObj.output_audio),
    },
    fields: { ...fields },
  };
}

function decodeProvider(
  key: string,
  fields: Record<string, unknown>
): CatalogProvider | null {
  const rawModels = isRecord(fields.models) ? fields.models : {};
  const models: CatalogModel[] = [];
  for (const [mid, rawModel] of Object.entries(rawModels)) {
    if (!isRecord(rawModel)) continue;
    const decoded = decodeModel(key, mid, rawModel);
    if (decoded !== null) models.push(decoded);
  }
  if (models.length === 0 && fields.id === undefined) return null;

  return {
    id: optString(fields.id) ?? key,
    name: optString(fields.name) ?? key,
    baseURL: nonEmpty(fields.baseURL),
    documentationURL: nonEmpty(fields.doc),
    environmentVariableNames: stringArray(fields.env),
    models: modelsInStableOrder(models),
    fields: { ...fields },
  };
}

export function decodeCatalog(input: Uint8Array | string): LingXiModelCatalog {
  const rawText =
    typeof input === "string" ? input : new TextDecoder("utf-8", { fatal: true }).decode(input);
  let root: unknown;
  try {
    root = JSON.parse(rawText);
  } catch (err) {
    throw new ModelCatalogError("invalid_catalog", `Invalid JSON: ${String(err)}`, { cause: err });
  }

  if (!isRecord(root)) {
    throw new ModelCatalogError("invalid_catalog", "Catalog root is not a JSON object");
  }
  if (!isRecord(root.providers) || Object.keys(root.providers).length === 0) {
    throw new ModelCatalogError("invalid_catalog", "providers missing or empty");
  }

  const schemaVersion = optString(root.schemaVersion) ?? optString(root.version) ?? "1.0";
  const major = schemaVersion.split(".", 1)[0];
  if (!SUPPORTED_MAJORS.has(major)) {
    throw new ModelCatalogError(
      "unsupported_schema_version",
      `Unsupported catalog schema version: ${schemaVersion}`,
      { schemaVersion }
    );
  }

  const generatedAt = parseTimestamp(root.generatedAt ?? root.updatedAt);
  const providers: CatalogProvider[] = [];
  for (const [pid, rawProv] of Object.entries(root.providers)) {
    if (!isRecord(rawProv)) continue;
    const decoded = decodeProvider(pid, rawProv);
    if (decoded !== null) providers.push(decoded);
  }
  if (providers.length === 0) {
    throw new ModelCatalogError("invalid_catalog", "No valid provider entries");
  }

  const sortedProviders = sortProviders(providers);
  const totalModels =
    optInt(root.totalModels) ??
    sortedProviders.reduce((acc, prov) => acc + prov.models.length, 0);
  const totalProviders = optInt(root.totalProviders) ?? sortedProviders.length;

  const revision: CatalogRevision = {
    schemaVersion,
    catalogRevision:
      optString(root.catalogRevision) ??
      optString(root.catalogHash) ??
      (generatedAt ? generatedAt.toISOString() : ""),
    catalogHash: optString(root.catalogHash),
    source: optString(root.source),
    sourceURL: optString(root.sourceURL),
    sourceHash: optString(root.sourceHash),
    generatedAt,
    sourceFetchedAt: parseTimestamp(root.sourceFetchedAt),
    totalProviders,
    totalModels,
  };

  const rawBytes = typeof input === "string" ? new TextEncoder().encode(input) : new Uint8Array(input);
  return new LingXiModelCatalog(revision, sortedProviders, rawBytes);
}
