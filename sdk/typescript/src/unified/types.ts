import type { NativeProtocol } from "../native/registry.ts";

export type LmpRole = "system" | "user" | "assistant" | "tool";

export type LmpContentBlock =
  | { readonly type: "text"; readonly text: string; readonly signature?: string }
  | { readonly type: "image_url"; readonly url: string }
  | { readonly type: "image_base64" | "audio"; readonly mediaType: string; readonly data: string }
  | { readonly type: "thinking"; readonly thinking: string; readonly signature?: string }
  | {
      readonly type: "tool_call";
      readonly id: string;
      readonly name: string;
      readonly arguments: Record<string, unknown>;
      readonly signature?: string;
    }
  | {
      readonly type: "tool_result";
      readonly toolCallID?: string;
      readonly toolCallId?: string;
      readonly content: string;
      readonly isError?: boolean;
    };

export interface LmpMessage {
  readonly role: LmpRole;
  readonly content: readonly LmpContentBlock[];
}

export interface LmpToolDefinition {
  readonly name: string;
  readonly description?: string;
  readonly parameters: Record<string, unknown>;
}

/** `auto` | `none` | `required` force a call to a specific function. */
export type LmpToolChoice = "auto" | "none" | "required" | { readonly tool: string };

export interface LmpThinkingConfig {
  readonly effort?: string;
  readonly budgetTokens?: number;
}

export type LmpResponseFormat =
  | { readonly type: "text" }
  | { readonly type: "json_object" }
  | {
      readonly type: "json_schema";
      readonly name: string;
      readonly schema: unknown;
      readonly strict?: boolean;
    };

export interface LmpRequest {
  readonly model: string;
  readonly messages: readonly LmpMessage[];
  readonly maxTokens?: number;
  readonly temperature?: number;
  readonly tools?: readonly LmpToolDefinition[];
  readonly toolChoice?: LmpToolChoice;
  readonly thinking?: LmpThinkingConfig;
  readonly responseFormat?: LmpResponseFormat;
  readonly providerExtensions?: Readonly<Record<string, unknown>>;
}

export interface LmpEmbeddingRequest {
  readonly model: string;
  readonly input: readonly string[];
}

export interface LmpEmbeddingResponse {
  readonly embeddings: readonly (readonly number[])[];
  readonly raw: Record<string, unknown>;
}

export interface LmpRerankRequest {
  readonly model: string;
  readonly query: string;
  readonly documents: readonly string[];
  readonly topN?: number;
}

export interface LmpRerankResponse {
  readonly results: readonly { readonly index: number; readonly relevanceScore: number }[];
  readonly raw: Record<string, unknown>;
}

export interface LmpUsage {
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly totalTokens: number;
  readonly reasoningTokens?: number;
  readonly cacheReadTokens?: number;
  readonly cacheWriteTokens?: number;
}

export interface LmpResponse {
  readonly id?: string;
  readonly model?: string;
  readonly content: readonly LmpContentBlock[];
  readonly finishReason?: string;
  readonly usage: LmpUsage;
  readonly raw: Record<string, unknown>;
}

/**
 * One normalized LMP stream event.
 *
 * `type` is snake_case; every other field is camelCase, matching the Rust core
 * and `specs/lmp-v1.md`. `message_start` and `message_end` bracket the stream,
 * `error` terminates it with a stable machine-readable `code`.
 */
export type LmpStreamEvent =
  | { readonly type: "message_start"; readonly id?: string }
  | { readonly type: "thinking_delta"; readonly delta: string; readonly signature?: string }
  | { readonly type: "text_delta"; readonly delta: string; readonly signature?: string }
  | {
      type: "tool_call_delta";
      index: number;
      id?: string;
      name?: string;
      argumentsDelta?: string;
      signature?: string;
    }
  | { readonly type: "usage"; readonly usage: LmpUsage }
  | { readonly type: "message_end"; readonly finishReason?: string }
  | { readonly type: "error"; readonly code: string; readonly message: string };

export class LmpUnsupportedFeatureError extends Error {
  /** Stable machine-readable code, matching the Rust core error taxonomy. */
  readonly code = "unsupported_feature";
  readonly protocol: NativeProtocol;
  readonly feature: string;

  constructor(protocol: NativeProtocol, feature: string) {
    super(`unsupported feature '${feature}' for protocol '${protocol}'`);
    this.name = "LmpUnsupportedFeatureError";
    this.protocol = protocol;
    this.feature = feature;
  }
}
