import { NativeClient } from "../native/client.ts";
import type { NativeProtocol } from "../native/registry.ts";
import type { SseEvent } from "../native/sse.ts";
import { StreamNormalizer, normalizeResponse, translateRequest } from "./adapters.ts";
import {
  LmpUnsupportedFeatureError,
  type LmpEmbeddingRequest,
  type LmpEmbeddingResponse,
  type LmpRequest,
  type LmpRerankRequest,
  type LmpRerankResponse,
  type LmpResponse,
  type LmpStreamEvent,
} from "./types.ts";

export class UnifiedClient {
  readonly native: NativeClient;

  constructor(nativeClient?: NativeClient) {
    this.native = nativeClient ?? new NativeClient();
  }

  async complete(
    protocol: NativeProtocol,
    baseURL: string,
    apiKey: string,
    req: LmpRequest
  ): Promise<LmpResponse> {
    const payload = translateRequest(protocol, req);
    const cfg = { baseURL, apiKey };
    let raw: Record<string, unknown>;
    switch (protocol) {
      case "openai_chat_completions":
        raw = await this.native.openaiChatCompletions(cfg, payload);
        break;
      case "openai_responses":
        raw = await this.native.openaiResponses(cfg, payload);
        break;
      case "anthropic_messages":
        raw = await this.native.anthropicMessages(cfg, payload);
        break;
      case "google_gemini":
        raw = await this.native.googleGeminiGenerate(cfg, req.model, payload);
        break;
      case "aws_bedrock":
        raw = await this.native.awsBedrockConverse(cfg, req.model, payload);
        break;
      case "google_vertex_ai":
        raw = await this.native.googleVertexGenerate(cfg, req.model, payload);
        break;
      case "azure_openai":
        raw = await this.native.azureOpenaiChatCompletions(cfg, req.model, payload);
        break;
      case "ollama":
        raw = await this.native.ollamaChat(cfg, payload);
        break;
      case "cohere":
        raw = await this.native.cohereChat(cfg, payload);
        break;
    }
    return normalizeResponse(protocol, raw);
  }

  /**
   * Streams normalized LMP events as they arrive.
   *
   * The provider frames are pulled on demand, so the consumer decides the pace:
   * breaking out of the loop closes the upstream response instead of draining it.
   * `message_end` is always emitted exactly once, including for protocols whose
   * upstream stream has no explicit terminal frame.
   */
  async *streamEach(
    protocol: NativeProtocol,
    baseURL: string,
    apiKey: string,
    req: LmpRequest
  ): AsyncGenerator<LmpStreamEvent> {
    const payload = translateRequest(protocol, req);
    const cfg = { baseURL, apiKey };
    const normalizer = new StreamNormalizer(protocol);
    for await (const event of this.streamGen(protocol, cfg, req.model, payload)) {
      for (const normalized of normalizer.push(event)) yield normalized;
    }
    for (const trailing of normalizer.finish()) yield trailing;
  }

  /** Buffers a whole stream; prefer {@link streamEach} for incremental delivery. */
  async stream(
    protocol: NativeProtocol,
    baseURL: string,
    apiKey: string,
    req: LmpRequest
  ): Promise<LmpStreamEvent[]> {
    const events: LmpStreamEvent[] = [];
    for await (const event of this.streamEach(protocol, baseURL, apiKey, req)) {
      events.push(event);
    }
    return events;
  }

  private streamGen(
    protocol: NativeProtocol,
    cfg: { baseURL: string; apiKey: string },
    model: string,
    payload: Record<string, unknown>
  ): AsyncGenerator<SseEvent> {
    switch (protocol) {
      case "openai_chat_completions":
        return this.native.streamOpenaiChatCompletions(cfg, payload);
      case "openai_responses":
        return this.native.streamOpenaiResponses(cfg, payload);
      case "anthropic_messages":
        return this.native.streamAnthropicMessages(cfg, payload);
      case "google_gemini":
        return this.native.streamGoogleGeminiGenerate(cfg, model, payload);
      case "aws_bedrock":
        return this.native.streamAwsBedrockConverse(cfg, model, payload);
      case "google_vertex_ai":
        return this.native.streamGoogleVertexGenerate(cfg, model, payload);
      case "azure_openai":
        return this.native.streamAzureOpenaiChatCompletions(cfg, model, payload);
      case "ollama":
        return this.native.streamOllamaChat(cfg, payload);
      case "cohere":
        return this.native.streamCohereChat(cfg, payload);
    }
  }

  async embed(
    protocol: NativeProtocol,
    baseURL: string,
    apiKey: string,
    req: LmpEmbeddingRequest
  ): Promise<LmpEmbeddingResponse> {
    if (req.input.length === 0) throw new Error("embedding input is empty");
    const base = baseURL.replace(/\/+$/, "");
    let url: string;
    let headers: Record<string, string> = {};
    let payload: Record<string, unknown>;
    switch (protocol) {
      case "openai_chat_completions":
        headers = { Authorization: `Bearer ${apiKey}` };
        url = `${base}/embeddings`;
        payload = { model: req.model, input: req.input };
        break;
      case "azure_openai":
        headers = { "api-key": apiKey };
        url = `${base}/openai/deployments/${req.model}/embeddings?api-version=2024-10-21`;
        payload = { input: req.input };
        break;
      case "google_gemini": {
        headers = { "x-goog-api-key": apiKey };
        const model = `models/${req.model.replace(/^models\//, "")}`;
        const content = (text: string) => ({ parts: [{ text }] });
        url = `${base}/v1beta/${model}:${req.input.length === 1 ? "embedContent" : "batchEmbedContents"}`;
        payload = req.input.length === 1
          ? { content: content(req.input[0]) }
          : { requests: req.input.map((text) => ({ model, content: content(text) })) };
        break;
      }
      case "ollama":
        url = `${base}/api/embed`;
        payload = { model: req.model, input: req.input };
        break;
      case "cohere":
        headers = { Authorization: `Bearer ${apiKey}` };
        url = `${base}/v2/embed`;
        payload = { model: req.model, texts: req.input, input_type: "search_document" };
        break;
      default:
        throw new LmpUnsupportedFeatureError(protocol, "embed");
    }
    const raw = await this.native.postJSONEndpoint(url, headers, payload);
    let embeddings: number[][] = [];
    if (protocol === "openai_chat_completions" || protocol === "azure_openai") {
      embeddings = ((raw.data as Array<{ embedding?: number[] }>) ?? []).map((d) => d.embedding ?? []);
    } else if (protocol === "google_gemini") {
      const rows = (Array.isArray(raw.embeddings) ? raw.embeddings : [raw.embedding]) as Array<{ values?: number[] } | undefined>;
      if (rows.length !== req.input.length) throw new Error("embedding count does not match request");
      embeddings = rows.map((row) => {
        const values = row?.values;
        if (!Array.isArray(values) || !values.length || !values.every(Number.isFinite)) {
          throw new Error("invalid embedding.values");
        }
        return values;
      });
    } else if (protocol === "ollama") {
      embeddings = (raw.embeddings as number[][]) ?? [];
    } else if (protocol === "cohere") {
      embeddings = ((raw.embeddings as { float?: number[][] } | undefined)?.float) ?? [];
    }
    return { embeddings, raw };
  }

  async rerank(
    protocol: NativeProtocol,
    baseURL: string,
    apiKey: string,
    req: LmpRerankRequest
  ): Promise<LmpRerankResponse> {
    if (protocol !== "cohere") {
      throw new LmpUnsupportedFeatureError(protocol, "rerank");
    }
    const base = baseURL.replace(/\/+$/, "");
    const raw = await this.native.postJSONEndpoint(
      `${base}/v2/rerank`,
      { Authorization: `Bearer ${apiKey}` },
      {
        model: req.model,
        query: req.query,
        documents: req.documents,
        ...(req.topN !== undefined ? { top_n: req.topN } : {}),
      }
    );
    const results = ((raw.results as Array<{ index?: number; relevance_score?: number }>) ?? []).map(
      (r) => ({ index: r.index ?? 0, relevanceScore: r.relevance_score ?? 0 })
    );
    return { results, raw };
  }
}
