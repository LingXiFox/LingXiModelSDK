import type { NativeProtocol } from "../native/registry.ts";

export type LmpRole = "system" | "user" | "assistant" | "tool";

export type LmpContentBlock =
  | { readonly type: "text"; readonly text: string }
  | { readonly type: "image_url"; readonly url: string }
  | { readonly type: "image_base64"; readonly mediaType: string; readonly data: string }
  | { readonly type: "thinking"; readonly thinking: string; readonly signature?: string }
  | {
      readonly type: "tool_call";
      readonly id: string;
      readonly name: string;
      readonly arguments: Record<string, unknown>;
    }
  | {
      readonly type: "tool_result";
      readonly toolCallID: string;
      readonly content: string;
      readonly isError?: boolean;
    };

export interface LmpMessage {
  readonly role: LmpRole;
  readonly content: readonly LmpContentBlock[];
}

export interface LmpRequest {
  readonly model: string;
  readonly messages: readonly LmpMessage[];
  readonly maxTokens?: number;
  readonly temperature?: number;
  readonly thinking?: { readonly effort?: string; readonly budgetTokens?: number };
  readonly responseFormat?:
    | { readonly type: "text" }
    | { readonly type: "json_object" }
    | { readonly type: "json_schema"; readonly name: string; readonly schema: unknown };
  readonly providerExtensions?: Readonly<Record<string, unknown>>;
}

export interface LmpUsage {
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly totalTokens: number;
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

export interface LmpStreamEvent {
  readonly type: "thinking_delta" | "text_delta" | "message_end";
  readonly delta?: string;
  readonly finishReason?: string;
}

export class LmpUnsupportedFeatureError extends Error {
  readonly protocol: NativeProtocol;
  readonly feature: string;

  constructor(protocol: NativeProtocol, feature: string) {
    super(`Unsupported feature '${feature}' for protocol '${protocol}'`);
    this.name = "LmpUnsupportedFeatureError";
    this.protocol = protocol;
    this.feature = feature;
  }
}
