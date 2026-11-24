import { ProviderRegistry, validateEndpointURL, type NativeProtocol } from "./registry.ts";
import { signSigV4 } from "./sigv4.ts";
import { parseNdjsonStream, parseSseStream, type SseEvent } from "./sse.ts";

export interface NativeRequestConfig {
  readonly baseURL: string;
  readonly apiKey?: string;
  readonly headers?: Record<string, string>;
  readonly allowInsecureHTTP?: boolean;
}

export class NativeHttpStatusError extends Error {
  readonly status: number;
  readonly body: string;

  constructor(status: number, body: string) {
    super(`Upstream HTTP ${status}: ${body}`);
    this.name = "NativeHttpStatusError";
    this.status = status;
    this.body = body;
  }
}

export class NativeClient {
  async openaiChatCompletions(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/chat/completions`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamOpenaiChatCompletions(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/chat/completions`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postSSE(url, headers, { ...payload, stream: true }, cfg.allowInsecureHTTP);
  }

  async openaiResponses(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/responses`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamOpenaiResponses(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/responses`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postSSE(url, headers, { ...payload, stream: true }, cfg.allowInsecureHTTP);
  }

  async anthropicMessages(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const url = joinAnthropic(cfg.baseURL);
    const headers = withHeaderAuth(cfg.headers, "x-api-key", cfg.apiKey);
    headers["anthropic-version"] ??= "2023-06-01";
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamAnthropicMessages(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const url = joinAnthropic(cfg.baseURL);
    const headers = withHeaderAuth(cfg.headers, "x-api-key", cfg.apiKey);
    headers["anthropic-version"] ??= "2023-06-01";
    return this.postSSE(url, headers, { ...payload, stream: true }, cfg.allowInsecureHTTP);
  }

  async googleGeminiGenerate(
    cfg: NativeRequestConfig,
    model: string,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/v1beta/models/${model}:generateContent`;
    const headers = withHeaderAuth(cfg.headers, "x-goog-api-key", cfg.apiKey);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamGoogleGeminiGenerate(
    cfg: NativeRequestConfig,
    model: string,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/v1beta/models/${model}:streamGenerateContent?alt=sse`;
    const headers = withHeaderAuth(cfg.headers, "x-goog-api-key", cfg.apiKey);
    return this.postSSE(url, headers, payload, cfg.allowInsecureHTTP);
  }

  async awsBedrockConverse(
    cfg: NativeRequestConfig,
    model: string,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const uri = `/model/${model}/converse`;
    const url = `${cfg.baseURL.replace(/\/+$/, "")}${uri}`;
    const headers = withBedrockAuth(url, uri, payload, cfg.apiKey, cfg.headers);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamAwsBedrockConverse(
    cfg: NativeRequestConfig,
    model: string,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const uri = `/model/${model}/converse-stream`;
    const url = `${cfg.baseURL.replace(/\/+$/, "")}${uri}`;
    const headers = withBedrockAuth(url, uri, payload, cfg.apiKey, cfg.headers);
    return this.postNDJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  async googleVertexGenerate(
    cfg: NativeRequestConfig,
    model: string,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const base = cfg.baseURL.replace(/\/+$/, "");
    const url = base.includes("/models")
      ? `${base}/${model}:generateContent`
      : `${base}/v1/publishers/google/models/${model}:generateContent`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamGoogleVertexGenerate(
    cfg: NativeRequestConfig,
    model: string,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const base = cfg.baseURL.replace(/\/+$/, "");
    const url = base.includes("/models")
      ? `${base}/${model}:streamGenerateContent?alt=sse`
      : `${base}/v1/publishers/google/models/${model}:streamGenerateContent?alt=sse`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postSSE(url, headers, payload, cfg.allowInsecureHTTP);
  }

  async azureOpenaiChatCompletions(
    cfg: NativeRequestConfig,
    deployment: string,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const apiVer = cfg.headers?.["x-azure-api-version"] ?? "2024-10-21";
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/openai/deployments/${deployment}/chat/completions?api-version=${apiVer}`;
    const headers = withHeaderAuth(cfg.headers, "api-key", cfg.apiKey);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamAzureOpenaiChatCompletions(
    cfg: NativeRequestConfig,
    deployment: string,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const apiVer = cfg.headers?.["x-azure-api-version"] ?? "2024-10-21";
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/openai/deployments/${deployment}/chat/completions?api-version=${apiVer}`;
    const headers = withHeaderAuth(cfg.headers, "api-key", cfg.apiKey);
    return this.postSSE(url, headers, { ...payload, stream: true }, cfg.allowInsecureHTTP);
  }

  async ollamaChat(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/api/chat`;
    return this.postJSON(url, cfg.headers ?? {}, { stream: false, ...payload }, cfg.allowInsecureHTTP);
  }

  streamOllamaChat(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/api/chat`;
    return this.postNDJSON(url, cfg.headers ?? {}, { ...payload, stream: true }, cfg.allowInsecureHTTP);
  }

  async cohereChat(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/v2/chat`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postJSON(url, headers, payload, cfg.allowInsecureHTTP);
  }

  streamCohereChat(
    cfg: NativeRequestConfig,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    const url = `${cfg.baseURL.replace(/\/+$/, "")}/v2/chat`;
    const headers = withBearer(cfg.headers, cfg.apiKey);
    return this.postSSE(url, headers, { ...payload, stream: true }, cfg.allowInsecureHTTP);
  }

  postJSONEndpoint(
    url: string,
    headers: Record<string, string>,
    payload: Record<string, unknown>,
    allowInsecureHTTP = false
  ): Promise<Record<string, unknown>> {
    return this.postJSON(url, headers, payload, allowInsecureHTTP);
  }

  async sendRegisteredJSON(
    registry: ProviderRegistry,
    providerID: string,
    protocol: NativeProtocol,
    options: {
      modelID?: string;
      apiKey?: string;
      endpointOverride?: string;
      payload: Record<string, unknown>;
    }
  ): Promise<Record<string, unknown>> {
    const { entry, baseURL } = registry.resolveEndpoint(
      providerID,
      protocol,
      options.modelID,
      options.endpointOverride,
      false
    );
    const headers: Record<string, string> = { ...(entry.defaultHeaders ?? {}) };
    if (options.modelID && entry.modelOverrides?.[options.modelID]?.defaultHeaders) {
      Object.assign(headers, entry.modelOverrides[options.modelID].defaultHeaders);
    }
    const cfg: NativeRequestConfig = { baseURL, apiKey: options.apiKey, headers };
    switch (protocol) {
      case "openai_chat_completions":
        return this.openaiChatCompletions(cfg, options.payload);
      case "openai_responses":
        return this.openaiResponses(cfg, options.payload);
      case "anthropic_messages":
        return this.anthropicMessages(cfg, options.payload);
      case "google_gemini":
        if (!options.modelID) throw new Error("google_gemini requires modelID");
        return this.googleGeminiGenerate(cfg, options.modelID, options.payload);
      case "aws_bedrock":
        if (!options.modelID) throw new Error("aws_bedrock requires modelID");
        return this.awsBedrockConverse(cfg, options.modelID, options.payload);
      case "google_vertex_ai":
        if (!options.modelID) throw new Error("google_vertex_ai requires modelID");
        return this.googleVertexGenerate(cfg, options.modelID, options.payload);
      case "azure_openai":
        if (!options.modelID) throw new Error("azure_openai requires modelID/deployment");
        return this.azureOpenaiChatCompletions(cfg, options.modelID, options.payload);
      case "ollama":
        return this.ollamaChat(cfg, options.payload);
      case "cohere":
        return this.cohereChat(cfg, options.payload);
    }
  }

  private async postJSON(
    url: string,
    headers: Record<string, string>,
    payload: Record<string, unknown>,
    allowInsecureHTTP = false
  ): Promise<Record<string, unknown>> {
    validateEndpointURL(url, allowInsecureHTTP);
    const resp = await fetch(url, {
      method: "POST",
      redirect: "error",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
        ...headers,
      },
      body: JSON.stringify(payload),
    });
    const text = await resp.text();
    if (!resp.ok) throw new NativeHttpStatusError(resp.status, text);
    return JSON.parse(text) as Record<string, unknown>;
  }

  private async *postSSE(
    url: string,
    headers: Record<string, string>,
    payload: Record<string, unknown>,
    allowInsecureHTTP = false
  ): AsyncGenerator<SseEvent> {
    validateEndpointURL(url, allowInsecureHTTP);
    const resp = await fetch(url, {
      method: "POST",
      redirect: "error",
      headers: {
        "Content-Type": "application/json",
        Accept: "text/event-stream",
        ...headers,
      },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      throw new NativeHttpStatusError(resp.status, await resp.text());
    }
    if (!resp.body) return;
    yield* parseSseStream(resp.body);
  }

  private async *postNDJSON(
    url: string,
    headers: Record<string, string>,
    payload: Record<string, unknown>,
    allowInsecureHTTP = false
  ): AsyncGenerator<SseEvent> {
    validateEndpointURL(url, allowInsecureHTTP);
    const resp = await fetch(url, {
      method: "POST",
      redirect: "error",
      headers: {
        "Content-Type": "application/json",
        Accept: "application/x-ndjson, application/json",
        ...headers,
      },
      body: JSON.stringify(payload),
    });
    if (!resp.ok) {
      throw new NativeHttpStatusError(resp.status, await resp.text());
    }
    if (!resp.body) return;
    yield* parseNdjsonStream(resp.body);
  }
}

function joinAnthropic(baseURL: string): string {
  const trimmed = baseURL.replace(/\/+$/, "");
  return trimmed.endsWith("/v1") ? `${trimmed}/messages` : `${trimmed}/v1/messages`;
}

function withBearer(
  headers: Record<string, string> | undefined,
  apiKey: string | undefined
): Record<string, string> {
  if (!apiKey) throw new Error("Bearer API key is required");
  return { Authorization: `Bearer ${apiKey}`, ...(headers ?? {}) };
}

function withHeaderAuth(
  headers: Record<string, string> | undefined,
  headerName: string,
  apiKey: string | undefined
): Record<string, string> {
  if (!apiKey) throw new Error(`${headerName} API key is required`);
  return { [headerName]: apiKey, ...(headers ?? {}) };
}

function withBedrockAuth(
  rawURL: string,
  canonicalURI: string,
  payload: Record<string, unknown>,
  apiKey: string | undefined,
  headers: Record<string, string> | undefined
): Record<string, string> {
  const out = { ...(headers ?? {}) };
  if (out.Authorization || out.authorization) return out;
  if (!apiKey) throw new Error("AWS Bedrock requires 'ACCESS_KEY:SECRET_KEY[:SESSION_TOKEN]'");
  const parts = apiKey.split(":");
  if (parts.length < 2) return withBearer(headers, apiKey);
  const u = new URL(rawURL);
  const region = out["x-aws-region"] ?? "us-east-1";
  const signed = signSigV4({
    method: "POST",
    canonicalURI,
    host: u.host,
    region,
    service: "bedrock",
    accessKey: parts[0],
    secretKey: parts[1],
    sessionToken: parts[2],
    payload: JSON.stringify(payload),
  });
  return { ...out, ...signed };
}
