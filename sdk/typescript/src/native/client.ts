import { ProviderRegistry, validateEndpointURL, type NativeProtocol } from "./registry.ts";
import { parseSseStream, type SseEvent } from "./sse.ts";

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
