import type { NativeProtocol } from "../native/registry.ts";
import type { SseEvent } from "../native/sse.ts";
import {
  LmpUnsupportedFeatureError,
  type LmpContentBlock,
  type LmpRequest,
  type LmpResponse,
  type LmpStreamEvent,
} from "./types.ts";

export function translateRequest(
  protocol: NativeProtocol,
  req: LmpRequest
): Record<string, unknown> {
  let payload: Record<string, unknown>;
  switch (protocol) {
    case "openai_chat_completions": {
      if (req.thinking?.budgetTokens !== undefined) {
        throw new LmpUnsupportedFeatureError(protocol, "budget_tokens");
      }
      payload = {
        model: req.model,
        messages: req.messages.map((m) => ({
          role: m.role,
          content: m.content
            .filter((b): b is { type: "text"; text: string } => b.type === "text")
            .map((b) => b.text)
            .join(""),
        })),
        ...(req.maxTokens !== undefined ? { max_tokens: req.maxTokens } : {}),
        ...(req.thinking?.effort ? { reasoning_effort: req.thinking.effort } : {}),
      };
      break;
    }
    case "openai_responses": {
      if (req.thinking?.budgetTokens !== undefined) {
        throw new LmpUnsupportedFeatureError(protocol, "budget_tokens");
      }
      payload = {
        model: req.model,
        input: req.messages.map((m) => ({
          role: m.role,
          content: m.content
            .filter((b): b is { type: "text"; text: string } => b.type === "text")
            .map((b) => ({ type: "input_text", text: b.text })),
        })),
        ...(req.maxTokens !== undefined ? { max_output_tokens: req.maxTokens } : {}),
      };
      break;
    }
    case "anthropic_messages": {
      if (
        req.responseFormat &&
        (req.responseFormat.type === "json_schema" || req.responseFormat.type === "json_object")
      ) {
        throw new LmpUnsupportedFeatureError(protocol, "json_schema");
      }
      const system = req.messages
        .filter((m) => m.role === "system")
        .flatMap((m) => m.content)
        .filter((b): b is { type: "text"; text: string } => b.type === "text")
        .map((b) => b.text)
        .join("\n");
      payload = {
        model: req.model,
        max_tokens: req.maxTokens ?? 4096,
        messages: req.messages
          .filter((m) => m.role !== "system")
          .map((m) => ({
            role: m.role === "assistant" ? "assistant" : "user",
            content: m.content
              .filter((b): b is { type: "text"; text: string } => b.type === "text")
              .map((b) => ({ type: "text", text: b.text })),
          })),
        ...(system ? { system } : {}),
        ...(req.thinking?.budgetTokens
          ? { thinking: { type: "enabled", budget_tokens: req.thinking.budgetTokens } }
          : {}),
      };
      break;
    }
    case "google_gemini": {
      payload = {
        contents: req.messages
          .filter((m) => m.role !== "system")
          .map((m) => ({
            role: m.role === "assistant" ? "model" : "user",
            parts: m.content
              .filter((b): b is { type: "text"; text: string } => b.type === "text")
              .map((b) => ({ text: b.text })),
          })),
      };
      break;
    }
  }
  if (req.providerExtensions) {
    Object.assign(payload, req.providerExtensions);
  }
  return payload;
}

export function normalizeResponse(
  protocol: NativeProtocol,
  raw: Record<string, unknown>
): LmpResponse {
  switch (protocol) {
    case "openai_chat_completions": {
      const choice = ((raw.choices as unknown[]) ?? [])[0] as Record<string, unknown>;
      const msg = (choice.message as Record<string, unknown>) ?? {};
      const content: LmpContentBlock[] = [];
      if (typeof msg.reasoning_content === "string" && msg.reasoning_content) {
        content.push({ type: "thinking", thinking: msg.reasoning_content });
      }
      if (typeof msg.content === "string" && msg.content) {
        content.push({ type: "text", text: msg.content });
      }
      const u = (raw.usage as Record<string, number>) ?? {};
      return {
        id: raw.id as string | undefined,
        model: raw.model as string | undefined,
        content,
        finishReason: choice.finish_reason as string | undefined,
        usage: {
          inputTokens: u.prompt_tokens ?? 0,
          outputTokens: u.completion_tokens ?? 0,
          totalTokens: u.total_tokens ?? 0,
        },
        raw,
      };
    }
    case "openai_responses": {
      const content: LmpContentBlock[] = [];
      for (const item of ((raw.output as Record<string, unknown>[]) ?? [])) {
        if (item.type === "reasoning" && Array.isArray(item.summary)) {
          const text = (item.summary as Record<string, unknown>[])
            .map((s) => String(s.text ?? ""))
            .filter(Boolean)
            .join("\n");
          if (text) content.push({ type: "thinking", thinking: text });
        } else if (item.type === "message" && Array.isArray(item.content)) {
          for (const p of item.content as Record<string, unknown>[]) {
            if (typeof p.text === "string" && p.text) {
              content.push({ type: "text", text: p.text });
            }
          }
        }
      }
      const u = (raw.usage as Record<string, number>) ?? {};
      return {
        id: raw.id as string | undefined,
        model: raw.model as string | undefined,
        content,
        finishReason: raw.status as string | undefined,
        usage: {
          inputTokens: u.input_tokens ?? 0,
          outputTokens: u.output_tokens ?? 0,
          totalTokens: u.total_tokens ?? 0,
        },
        raw,
      };
    }
    case "anthropic_messages": {
      const content: LmpContentBlock[] = [];
      for (const b of ((raw.content as Record<string, unknown>[]) ?? [])) {
        if (b.type === "thinking") {
          content.push({
            type: "thinking",
            thinking: String(b.thinking ?? ""),
            signature: typeof b.signature === "string" ? b.signature : undefined,
          });
        } else if (b.type === "text") {
          content.push({ type: "text", text: String(b.text ?? "") });
        }
      }
      const u = (raw.usage as Record<string, number>) ?? {};
      const inTok = u.input_tokens ?? 0;
      const outTok = u.output_tokens ?? 0;
      return {
        id: raw.id as string | undefined,
        model: raw.model as string | undefined,
        content,
        finishReason: raw.stop_reason as string | undefined,
        usage: {
          inputTokens: inTok,
          outputTokens: outTok,
          totalTokens: inTok + outTok,
          cacheReadTokens: u.cache_read_input_tokens,
          cacheWriteTokens: u.cache_creation_input_tokens,
        },
        raw,
      };
    }
    case "google_gemini": {
      const cand = ((raw.candidates as Record<string, unknown>[]) ?? [])[0] ?? {};
      const parts = ((cand.content as Record<string, unknown>)?.parts as Record<string, unknown>[]) ?? [];
      const content: LmpContentBlock[] = [];
      for (const p of parts) {
        if (typeof p.text === "string" && p.text) {
          content.push(
            p.thought === true
              ? { type: "thinking", thinking: p.text }
              : { type: "text", text: p.text }
          );
        }
      }
      const u = (raw.usageMetadata as Record<string, number>) ?? {};
      return {
        content,
        finishReason: cand.finishReason as string | undefined,
        usage: {
          inputTokens: u.promptTokenCount ?? 0,
          outputTokens: u.candidatesTokenCount ?? 0,
          totalTokens: u.totalTokenCount ?? 0,
        },
        raw,
      };
    }
  }
}

export function normalizeStreamEvents(
  protocol: NativeProtocol,
  events: readonly SseEvent[]
): LmpStreamEvent[] {
  const out: LmpStreamEvent[] = [];
  for (const ev of events) {
    const trimmed = ev.data.trim();
    if (!trimmed || trimmed === "[DONE]") continue;
    const val = JSON.parse(trimmed) as Record<string, unknown>;
    if (protocol === "openai_chat_completions") {
      const c0 = ((val.choices as Record<string, unknown>[]) ?? [])[0];
      if (!c0) continue;
      const d = (c0.delta as Record<string, unknown>) ?? {};
      if (typeof d.reasoning_content === "string" && d.reasoning_content) {
        out.push({ type: "thinking_delta", delta: d.reasoning_content });
      }
      if (typeof d.content === "string" && d.content) {
        out.push({ type: "text_delta", delta: d.content });
      }
      if (typeof c0.finish_reason === "string" && c0.finish_reason) {
        out.push({ type: "message_end", finishReason: c0.finish_reason });
      }
    } else if (protocol === "openai_responses") {
      const kind = ev.event ?? (val.type as string);
      if (kind === "response.output_text.delta" && typeof val.delta === "string") {
        out.push({ type: "text_delta", delta: val.delta });
      } else if (kind === "response.completed") {
        out.push({ type: "message_end", finishReason: "completed" });
      }
    } else if (protocol === "anthropic_messages") {
      const kind = ev.event ?? (val.type as string);
      if (kind === "content_block_delta") {
        const d = (val.delta as Record<string, unknown>) ?? {};
        if (d.type === "thinking_delta" && typeof d.thinking === "string") {
          out.push({ type: "thinking_delta", delta: d.thinking });
        } else if (d.type === "text_delta" && typeof d.text === "string") {
          out.push({ type: "text_delta", delta: d.text });
        }
      } else if (kind === "message_stop") {
        out.push({ type: "message_end", finishReason: "stop" });
      }
    } else if (protocol === "google_gemini") {
      const c0 = ((val.candidates as Record<string, unknown>[]) ?? [])[0] ?? {};
      const parts = ((c0.content as Record<string, unknown>)?.parts as Record<string, unknown>[]) ?? [];
      for (const p of parts) {
        if (typeof p.text === "string" && p.text) {
          out.push({
            type: p.thought === true ? "thinking_delta" : "text_delta",
            delta: p.text,
          });
        }
      }
      if (typeof c0.finishReason === "string" && c0.finishReason) {
        out.push({ type: "message_end", finishReason: c0.finishReason });
      }
    }
  }
  return out;
}
