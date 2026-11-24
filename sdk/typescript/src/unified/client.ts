import { NativeClient } from "../native/client.ts";
import type { NativeProtocol } from "../native/registry.ts";
import { normalizeResponse, normalizeStreamEvents, translateRequest } from "./adapters.ts";
import type { LmpRequest, LmpResponse, LmpStreamEvent } from "./types.ts";

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
    }
    return normalizeResponse(protocol, raw);
  }

  async stream(
    protocol: NativeProtocol,
    baseURL: string,
    apiKey: string,
    req: LmpRequest
  ): Promise<LmpStreamEvent[]> {
    const payload = translateRequest(protocol, req);
    const cfg = { baseURL, apiKey };
    const collected = [];
    const gen =
      protocol === "openai_chat_completions"
        ? this.native.streamOpenaiChatCompletions(cfg, payload)
        : protocol === "openai_responses"
          ? this.native.streamOpenaiResponses(cfg, payload)
          : protocol === "anthropic_messages"
            ? this.native.streamAnthropicMessages(cfg, payload)
            : this.native.streamGoogleGeminiGenerate(cfg, req.model, payload);
    for await (const ev of gen) {
      collected.push(ev);
    }
    return normalizeStreamEvents(protocol, collected);
  }
}
