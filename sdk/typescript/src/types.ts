export type CatalogModelStatus = "active" | "preview" | "deprecated" | "retired" | "unknown";

export function parseModelStatus(raw: unknown): CatalogModelStatus {
  if (raw === "deprecated" || raw === "retired") return "deprecated";
  if (raw === "beta" || raw === "preview") return "preview";
  if (raw === "active") return "active";
  return "unknown";
}

export function isStatusSelectable(status: CatalogModelStatus): boolean {
  return status !== "deprecated" && status !== "retired";
}

export interface ModelLimits {
  readonly context: number | null;
  readonly input: number | null;
  readonly output: number | null;
}

export interface ModelCapabilities {
  readonly reasoning: boolean;
  readonly reasoningEfforts: readonly string[];
  readonly toolCalling: boolean;
  readonly structuredOutput: boolean;
  readonly attachments: boolean;
  readonly vision: boolean;
  readonly audioInput: boolean;
  readonly audioOutput: boolean;
  readonly inputModalities: readonly string[];
  readonly outputModalities: readonly string[];
  readonly openWeights: boolean;
  readonly supportsTemperature: boolean | null;
}

export interface ModelPricing {
  readonly input: number | null;
  readonly output: number | null;
  readonly cacheRead: number | null;
  readonly cacheWrite: number | null;
  readonly reasoning: number | null;
  readonly inputAudio: number | null;
  readonly outputAudio: number | null;
}

export function isPricingUnknown(pricing: ModelPricing): boolean {
  return (
    pricing.input === null &&
    pricing.output === null &&
    pricing.cacheRead === null &&
    pricing.cacheWrite === null &&
    pricing.reasoning === null &&
    pricing.inputAudio === null &&
    pricing.outputAudio === null
  );
}

export interface CatalogModel {
  readonly id: string;
  readonly providerID: string;
  readonly qualifiedID: string;
  readonly name: string;
  readonly family: string | null;
  readonly overview: string | null;
  readonly releaseDate: string | null;
  readonly lastUpdated: string | null;
  readonly knowledgeCutoff: string | null;
  readonly canonicalModelID: string | null;
  readonly status: CatalogModelStatus;
  readonly limits: ModelLimits;
  readonly capabilities: ModelCapabilities;
  readonly pricing: ModelPricing;
  readonly fields: Readonly<Record<string, unknown>>;
}

export interface CatalogProvider {
  readonly id: string;
  readonly name: string;
  readonly baseURL: string | null;
  readonly documentationURL: string | null;
  readonly environmentVariableNames: readonly string[];
  readonly models: readonly CatalogModel[];
  readonly fields: Readonly<Record<string, unknown>>;
}

export interface CatalogRevision {
  readonly schemaVersion: string;
  readonly catalogRevision: string;
  readonly catalogHash: string | null;
  readonly source: string | null;
  readonly sourceURL: string | null;
  readonly sourceHash: string | null;
  readonly generatedAt: Date | null;
  readonly sourceFetchedAt: Date | null;
  readonly totalProviders: number;
  readonly totalModels: number;
}

export interface ModelFilter {
  readonly providerID?: string;
  readonly query?: string;
  readonly reasoning?: boolean;
  readonly toolCalling?: boolean;
  readonly structuredOutput?: boolean;
  readonly vision?: boolean;
  readonly attachments?: boolean;
  readonly openWeights?: boolean;
  readonly minimumContextWindow?: number;
  readonly minimumMaxOutputTokens?: number;
  readonly includeDeprecated?: boolean;
  readonly featuredProviderIDs?: readonly string[];
}
