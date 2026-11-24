export type NativeProtocol =
  | "openai_chat_completions"
  | "openai_responses"
  | "anthropic_messages"
  | "google_gemini"
  | "aws_bedrock"
  | "google_vertex_ai"
  | "azure_openai"
  | "ollama"
  | "cohere";

export interface AuthConfig {
  readonly type: string;
  readonly headerName?: string;
  readonly prefix?: string;
  readonly queryParam?: string;
  readonly env?: readonly string[];
}

export interface ProviderEntry {
  readonly id: string;
  readonly name: string;
  readonly protocols: readonly string[];
  readonly officialEndpoints: Readonly<Record<string, string>>;
  readonly auth: AuthConfig;
  readonly defaultHeaders?: Readonly<Record<string, string>>;
  readonly extraParams?: Readonly<Record<string, unknown>>;
  readonly modelOverrides?: Readonly<
    Record<string, { endpoint?: string; defaultHeaders?: Record<string, string> }>
  >;
  readonly verificationStatus: string;
}

export function validateEndpointURL(rawURL: string, allowInsecureHTTP = false): void {
  const u = new URL(rawURL);
  if (u.protocol === "https:") return;
  if (u.protocol === "http:") {
    const isLoopback =
      u.hostname === "127.0.0.1" || u.hostname === "localhost" || u.hostname === "::1" || u.hostname === "[::1]";
    if (isLoopback || allowInsecureHTTP) return;
    throw new Error(`Non-HTTPS endpoint '${rawURL}' is disallowed for non-loopback hosts`);
  }
  throw new Error(`Unsupported URL scheme '${u.protocol}'`);
}

export class ProviderRegistry {
  private readonly providers: Readonly<Record<string, ProviderEntry>>;

  constructor(providers: Readonly<Record<string, ProviderEntry>>) {
    this.providers = providers;
  }

  static fromJSON(jsonText: string): ProviderRegistry {
    const doc = JSON.parse(jsonText) as {
      schemaVersion?: string;
      providers?: Record<string, ProviderEntry>;
    };
    if (doc.schemaVersion !== "1.0" || !doc.providers) {
      throw new Error(`Unsupported registry schemaVersion: ${String(doc.schemaVersion)}`);
    }
    return new ProviderRegistry(doc.providers);
  }

  get size(): number {
    return Object.keys(this.providers).length;
  }

  get(providerID: string): ProviderEntry | undefined {
    return this.providers[providerID];
  }

  resolveEndpoint(
    providerID: string,
    protocol: NativeProtocol,
    modelID?: string,
    endpointOverride?: string,
    allowInsecureHTTP = false
  ): { entry: ProviderEntry; baseURL: string } {
    const entry = this.get(providerID);
    if (!entry) throw new Error(`Unknown provider '${providerID}'`);
    if (!entry.protocols.includes(protocol)) {
      throw new Error(`Provider '${providerID}' does not declare protocol '${protocol}'`);
    }
    const modelBase = modelID ? entry.modelOverrides?.[modelID]?.endpoint : undefined;
    const rawBase = endpointOverride ?? modelBase ?? entry.officialEndpoints[protocol];
    if (!rawBase) {
      throw new Error(`No endpoint configured for ${providerID}/${protocol}`);
    }
    const baseURL = rawBase.replace(/\/+$/, "");
    validateEndpointURL(baseURL, allowInsecureHTTP);
    return { entry, baseURL };
  }
}
