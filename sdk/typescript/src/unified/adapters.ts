import type { NativeProtocol } from "../native/registry.ts";
import type { SseEvent } from "../native/sse.ts";
import {
  LmpUnsupportedFeatureError,
  type LmpContentBlock,
  type LmpRequest,
  type LmpResponse,
  type LmpResponseFormat,
  type LmpStreamEvent,
  type LmpToolChoice,
  type LmpUsage,
} from "./types.ts";

type JSONObject = Record<string, unknown>;

function unsupported(protocol: NativeProtocol, feature: string): never {
  throw new LmpUnsupportedFeatureError(protocol, feature);
}

function asRecord(value: unknown): JSONObject | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as JSONObject)
    : undefined;
}

/** Walks a nested object path, returning undefined when a step is missing. */
function lookup(raw: unknown, ...path: string[]): unknown {
  let current: unknown = raw;
  for (const key of path) {
    const record = asRecord(current);
    if (record === undefined) return undefined;
    current = record[key];
  }
  return current;
}

function strAt(raw: unknown, ...path: string[]): string {
  const value = lookup(raw, ...path);
  return typeof value === "string" ? value : "";
}

function objAt(raw: unknown, ...path: string[]): JSONObject | undefined {
  return asRecord(lookup(raw, ...path));
}

function arrAt(raw: unknown, ...path: string[]): unknown[] {
  const value = lookup(raw, ...path);
  return Array.isArray(value) ? value : [];
}

function numAt(raw: unknown, ...path: string[]): number | undefined {
  const value = lookup(raw, ...path);
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

function intOr(raw: unknown, ...path: string[]): number {
  return numAt(raw, ...path) ?? 0;
}

function optInt(raw: unknown, ...path: string[]): number | undefined {
  const value = numAt(raw, ...path);
  return value === undefined ? undefined : Math.trunc(value);
}

/** Accepts both the object form (Ollama) and the JSON string form (everyone else). */
function argumentsObject(raw: unknown): JSONObject {
  if (typeof raw === "string") {
    if (raw.trim() === "") return {};
    try {
      return asRecord(JSON.parse(raw)) ?? {};
    } catch {
      return {};
    }
  }
  return asRecord(raw) ?? {};
}

function argumentsString(args: JSONObject): string {
  return JSON.stringify(args ?? {});
}

function toolCallId(block: Extract<LmpContentBlock, { type: "tool_result" }>): string {
  return block.toolCallId ?? block.toolCallID ?? "";
}

function strictValue(strict: boolean | undefined): boolean {
  return strict === undefined ? true : strict;
}

function openAITools(tools: readonly { name: string; description?: string; parameters: JSONObject }[]): JSONObject[] {
  return tools.map((tool) => ({
    type: "function",
    function: { name: tool.name, description: tool.description, parameters: tool.parameters },
  }));
}

function openAIToolChoice(choice: LmpToolChoice | undefined): unknown {
  if (choice === undefined) return undefined;
  if (choice === "auto" || choice === "none" || choice === "required") return choice;
  return { type: "function", function: { name: choice.tool } };
}

/**
 * Maps an LMP response format to the OpenAI `response_format` object.
 *
 * `strict` defaults to true, matching the Rust core: a schema advertised as
 * structured output should not silently degrade to best-effort.
 */
function openAIResponseFormat(format: LmpResponseFormat | undefined): unknown {
  if (format === undefined) return undefined;
  switch (format.type) {
    case "text":
      return { type: "text" };
    case "json_object":
      return { type: "json_object" };
    case "json_schema":
      return {
        type: "json_schema",
        json_schema: { name: format.name, schema: format.schema, strict: strictValue(format.strict) },
      };
  }
}
/**
 * Translates an LMP request into the native wire payload of one protocol.
 *
 * Every mapping decision lives here so the adapters stay the single source of
 * truth, mirroring the Rust core (`core/src/lmp/adapters.rs`).
 */
export function translateRequest(
  protocol: NativeProtocol,
  req: LmpRequest
): JSONObject {
  let payload: JSONObject;
  switch (protocol) {
    case "openai_chat_completions":
    case "azure_openai":
      payload = toOpenAIChat(protocol, req);
      break;
    case "openai_responses":
      payload = toOpenAIResponses(req);
      break;
    case "anthropic_messages":
      payload = toAnthropicMessages(req);
      break;
    case "google_gemini":
    case "google_vertex_ai":
      payload = toGoogle(protocol, req);
      break;
    case "aws_bedrock":
      payload = toAWSBedrock(req);
      break;
    case "ollama":
      payload = toOllamaChat(req);
      break;
    case "cohere":
      payload = toCohereChat(req);
      break;
  }
  if (req.providerExtensions) {
    Object.assign(payload, req.providerExtensions);
  }
  return payload;
}

function toOpenAIChat(protocol: NativeProtocol, req: LmpRequest): JSONObject {
  if (req.thinking?.budgetTokens !== undefined) {
    return unsupported(protocol, "budget_tokens");
  }
  const messages: JSONObject[] = [];
  for (const message of req.messages) {
    let text = "";
    const parts: JSONObject[] = [];
    let hasNonText = false;
    const toolCalls: JSONObject[] = [];
    // OpenAI Chat carries one tool result per message: every tool_result block
    // becomes its own role=tool message. Folding them into the surrounding turn
    // would drop every call id but the last.
    const toolResults: Array<{ id: string; content: string }> = [];
    for (const block of message.content) {
      switch (block.type) {
        case "text":
          text += block.text;
          parts.push({ type: "text", text: block.text });
          break;
        case "image_url":
          hasNonText = true;
          parts.push({ type: "image_url", image_url: { url: block.url } });
          break;
        case "image_base64":
          hasNonText = true;
          parts.push({
            type: "image_url",
            image_url: { url: `data:${block.mediaType};base64,${block.data}` },
          });
          break;
        case "tool_call":
          toolCalls.push({
            id: block.id,
            type: "function",
            function: { name: block.name, arguments: argumentsString(block.arguments) },
          });
          break;
        case "tool_result":
          toolResults.push({ id: toolCallId(block), content: block.content });
          break;
        case "audio":
          return unsupported(protocol, "audio_block");
        case "thinking":
          break;
      }
    }
    if (text !== "" || hasNonText || toolCalls.length > 0) {
      const entry: JSONObject = { role: message.role };
      entry.content = hasNonText ? parts : text;
      if (toolCalls.length > 0) entry.tool_calls = toolCalls;
      messages.push(entry);
    }
    for (const result of toolResults) {
      messages.push({ role: "tool", tool_call_id: result.id, content: result.content });
    }
  }
  const out: JSONObject = { model: req.model, messages };
  if (req.maxTokens !== undefined) out.max_tokens = req.maxTokens;
  if (req.temperature !== undefined) out.temperature = req.temperature;
  if (req.thinking?.effort) out.reasoning_effort = req.thinking.effort;
  if (req.tools && req.tools.length > 0) out.tools = openAITools(req.tools);
  const choice = openAIToolChoice(req.toolChoice);
  if (choice !== undefined) out.tool_choice = choice;
  const format = openAIResponseFormat(req.responseFormat);
  if (format !== undefined) out.response_format = format;
  return out;
}

function toOpenAIResponses(req: LmpRequest): JSONObject {
  if (req.thinking?.budgetTokens !== undefined) {
    return unsupported("openai_responses", "budget_tokens");
  }
  const input: JSONObject[] = [];
  for (const message of req.messages) {
    // Responses replays tool output as function_call_output items, which live
    // in the user turn rather than a distinct tool role.
    const role = message.role === "tool" ? "user" : message.role;
    // Reasoning and function_call items must be emitted in their original order
    // relative to the surrounding text, or a stateless replay loses the reasoning
    // state and the call it produced.
    let parts: JSONObject[] = [];
    const flush = (): void => {
      if (parts.length > 0) {
        input.push({ role, content: parts });
        parts = [];
      }
    };
    for (const block of message.content) {
      switch (block.type) {
        case "text":
          parts.push({ type: "input_text", text: block.text });
          break;
        case "image_url":
          parts.push({ type: "input_image", image_url: block.url });
          break;
        case "image_base64":
          parts.push({
            type: "input_image",
            image_url: `data:${block.mediaType};base64,${block.data}`,
          });
          break;
        case "thinking": {
          flush();
          // `signature` carries the opaque encrypted_content token so a
          // store=false replay keeps working reasoning state.
          const item: JSONObject = {
            type: "reasoning",
            summary: [{ type: "summary_text", text: block.thinking }],
          };
          if (block.signature !== undefined && block.signature !== "") {
            item.encrypted_content = block.signature;
          }
          input.push(item);
          break;
        }
        case "tool_call":
          flush();
          input.push({
            type: "function_call",
            call_id: block.id,
            name: block.name,
            arguments: argumentsString(block.arguments),
          });
          break;
        case "tool_result":
          input.push({
            type: "function_call_output",
            call_id: toolCallId(block),
            output: block.content,
          });
          break;
        case "audio":
          return unsupported("openai_responses", "audio_block");
      }
    }
    flush();
  }
  const out: JSONObject = { model: req.model, input };
  if (req.maxTokens !== undefined) out.max_output_tokens = req.maxTokens;
  if (req.thinking?.effort) out.reasoning = { effort: req.thinking.effort };
  if (req.responseFormat) {
    switch (req.responseFormat.type) {
      case "text":
        break;
      case "json_object":
        out.text = { format: { type: "json_object" } };
        break;
      case "json_schema":
        out.text = {
          format: {
            type: "json_schema",
            name: req.responseFormat.name,
            schema: req.responseFormat.schema,
            strict: strictValue(req.responseFormat.strict),
          },
        };
        break;
    }
  }
  return out;
}

function toAnthropicMessages(req: LmpRequest): JSONObject {
  if (
    req.responseFormat &&
    (req.responseFormat.type === "json_schema" || req.responseFormat.type === "json_object")
  ) {
    return unsupported("anthropic_messages", "json_schema");
  }
  // Anthropic rejects forced tool use (`any` / `tool`) together with manual
  // extended thinking; report it here instead of forwarding a request the
  // provider answers with an opaque 400.
  const forcedChoice = req.toolChoice === "required" || (req.toolChoice !== undefined && typeof req.toolChoice === "object");
  const hasBudget = (req.thinking?.budgetTokens ?? 0) > 0;
  if (hasBudget && forcedChoice) {
    return unsupported("anthropic_messages", "forced_tool_choice_with_thinking");
  }
  const system: string[] = [];
  const messages: JSONObject[] = [];
  for (const message of req.messages) {
    if (message.role === "system") {
      for (const block of message.content) {
        if (block.type === "text") system.push(block.text);
      }
      continue;
    }
    const role = message.role === "assistant" ? "assistant" : "user";
    const blocks: JSONObject[] = [];
    for (const block of message.content) {
      switch (block.type) {
        case "text":
          blocks.push({ type: "text", text: block.text });
          break;
        case "image_base64":
          blocks.push({
            type: "image",
            source: { type: "base64", media_type: block.mediaType, data: block.data },
          });
          break;
        case "image_url":
          blocks.push({ type: "image", source: { type: "url", url: block.url } });
          break;
        case "thinking": {
          const entry: JSONObject = { type: "thinking", thinking: block.thinking };
          if (block.signature !== undefined && block.signature !== "") {
            entry.signature = block.signature;
          }
          blocks.push(entry);
          break;
        }
        case "tool_call":
          blocks.push({ type: "tool_use", id: block.id, name: block.name, input: block.arguments });
          break;
        case "tool_result":
          blocks.push({
            type: "tool_result",
            tool_use_id: toolCallId(block),
            content: block.content,
            is_error: block.isError === true,
          });
          break;
        case "audio":
          return unsupported("anthropic_messages", "audio_block");
      }
    }
    // Anthropic expects every tool_result for one assistant turn inside a single
    // user message; consecutive same-role turns are merged so a caller that emits
    // one LMP tool message per result still gets one.
    const last = messages[messages.length - 1];
    if (last !== undefined && last.role === role && Array.isArray(last.content)) {
      (last.content as JSONObject[]).push(...blocks);
      continue;
    }
    messages.push({ role, content: blocks });
  }
  const out: JSONObject = {
    model: req.model,
    max_tokens: req.maxTokens !== undefined && req.maxTokens > 0 ? req.maxTokens : 4096,
    messages,
  };
  if (system.length > 0) out.system = system.join("\n");
  if (req.temperature !== undefined) out.temperature = req.temperature;
  if (hasBudget) {
    out.thinking = { type: "enabled", budget_tokens: req.thinking?.budgetTokens };
  }
  // Anthropic steers thinking depth through output_config.effort on models that
  // support it; there is no per-request budget equivalent for effort.
  if (req.thinking?.effort) out.output_config = { effort: req.thinking.effort };
  if (req.tools && req.tools.length > 0) {
    out.tools = req.tools.map((tool) => ({
      name: tool.name,
      description: tool.description,
      input_schema: tool.parameters,
    }));
  }
  if (req.toolChoice !== undefined) {
    if (req.toolChoice === "auto") out.tool_choice = { type: "auto" };
    else if (req.toolChoice === "none") out.tool_choice = { type: "none" };
    else if (req.toolChoice === "required") out.tool_choice = { type: "any" };
    else out.tool_choice = { type: "tool", name: req.toolChoice.tool };
  }
  return out;
}

function toGoogle(protocol: NativeProtocol, req: LmpRequest): JSONObject {
  const system: JSONObject[] = [];
  const contents: { role: string; parts: JSONObject[] }[] = [];
  const calls = new Map<string, { name: string; wireID: boolean }>();
  let previousRole: string | undefined;
  for (const message of req.messages) {
    if (message.role === "system") {
      for (const block of message.content) {
        if (block.type !== "text" || block.signature !== undefined) {
          return unsupported(protocol, "system_content");
        }
        system.push({ text: block.text });
      }
      continue;
    }
    if (message.role === "assistant") calls.clear();
    const parts: JSONObject[] = [];
    for (const block of message.content) {
      let part: JSONObject;
      switch (block.type) {
        case "text":
          part = { text: block.text };
          break;
        case "thinking":
          part = { thought: true, text: block.thinking };
          break;
        case "image_base64":
        case "audio":
          part = { inlineData: { mimeType: block.mediaType, data: block.data } };
          break;
        case "image_url":
          // Gemini needs a mimeType for remote media, which LMP does not carry.
          return unsupported(protocol, "image_url");
        case "tool_call": {
          if (calls.has(block.id)) return unsupported(protocol, "duplicate_tool_call_id");
          const wireID = !!block.id && block.id !== block.name && !block.id.startsWith("gemini-part:");
          calls.set(block.id, { name: block.name, wireID });
          part = {
            functionCall: { name: block.name, args: block.arguments, ...(wireID ? { id: block.id } : {}) },
          };
          break;
        }
        case "tool_result": {
          const id = toolCallId(block);
          const call = calls.get(id);
          if (call === undefined) return unsupported(protocol, "unmatched_tool_result");
          part = {
            functionResponse: {
              name: call.name,
              response: { [block.isError ? "error" : "content"]: block.content },
              ...(call.wireID ? { id } : {}),
            },
          };
          break;
        }
      }
      if ("signature" in block && block.signature !== undefined && block.signature !== "") {
        part.thoughtSignature = block.signature;
      }
      parts.push(part);
    }
    if (message.role === "tool" && previousRole === "tool" && contents.length > 0) {
      contents[contents.length - 1].parts.push(...parts);
    } else {
      contents.push({ role: message.role === "assistant" ? "model" : "user", parts });
    }
    previousRole = message.role;
  }
  const generationConfig: JSONObject = {};
  if (req.maxTokens !== undefined) generationConfig.maxOutputTokens = req.maxTokens;
  if (req.temperature !== undefined) generationConfig.temperature = req.temperature;
  if (req.responseFormat?.type === "json_object" || req.responseFormat?.type === "json_schema") {
    generationConfig.responseMimeType = "application/json";
    if (req.responseFormat.type === "json_schema") generationConfig.responseSchema = req.responseFormat.schema;
  }
  const effort = req.thinking?.effort;
  const budgetTokens = req.thinking?.budgetTokens;
  if (effort !== undefined && budgetTokens !== undefined) {
    return unsupported(protocol, "thinking_budget_and_effort");
  }
  if (budgetTokens !== undefined) {
    generationConfig.thinkingConfig = { thinkingBudget: budgetTokens, includeThoughts: true };
  }
  if (effort !== undefined) {
    if (!["minimal", "low", "medium", "high"].includes(effort)) {
      return unsupported(protocol, "thinking_effort");
    }
    generationConfig.thinkingConfig = { thinkingLevel: effort.toUpperCase(), includeThoughts: true };
  }
  const out: JSONObject = { contents };
  if (system.length > 0) out.systemInstruction = { parts: system };
  if (Object.keys(generationConfig).length > 0) out.generationConfig = generationConfig;
  if (req.tools && req.tools.length > 0) {
    out.tools = [
      {
        functionDeclarations: req.tools.map((tool) => ({
          name: tool.name,
          description: tool.description,
          parameters: tool.parameters,
        })),
      },
    ];
  }
  if (req.toolChoice !== undefined) {
    if (!req.tools || req.tools.length === 0) {
      return unsupported(protocol, "tool_choice_without_tools");
    }
    // Gemini forces a call through toolConfig; `any` can be narrowed to a single
    // function via allowedFunctionNames.
    const config: JSONObject = {};
    if (req.toolChoice === "auto") config.mode = "AUTO";
    else if (req.toolChoice === "none") config.mode = "NONE";
    else if (req.toolChoice === "required") config.mode = "ANY";
    else {
      config.mode = "ANY";
      config.allowedFunctionNames = [req.toolChoice.tool];
    }
    out.toolConfig = { functionCallingConfig: config };
  }
  return out;
}

function bedrockImageFormat(mediaType: string): string | undefined {
  switch (mediaType) {
    case "image/png":
      return "png";
    case "image/jpeg":
      return "jpeg";
    case "image/gif":
      return "gif";
    case "image/webp":
      return "webp";
    default:
      return undefined;
  }
}

function toAWSBedrock(req: LmpRequest): JSONObject {
  const system: JSONObject[] = [];
  const messages: JSONObject[] = [];
  for (const message of req.messages) {
    if (message.role === "system") {
      for (const block of message.content) {
        if (block.type === "text") system.push({ text: block.text });
      }
      continue;
    }
    const role = message.role === "assistant" ? "assistant" : "user";
    const blocks: JSONObject[] = [];
    for (const block of message.content) {
      switch (block.type) {
        case "text":
          blocks.push({ text: block.text });
          break;
        case "thinking": {
          const reasoning: JSONObject = { text: block.thinking };
          if (block.signature !== undefined && block.signature !== "") {
            reasoning.signature = block.signature;
          }
          blocks.push({ reasoningContent: { reasoningText: reasoning } });
          break;
        }
        case "tool_call":
          blocks.push({ toolUse: { toolUseId: block.id, name: block.name, input: block.arguments } });
          break;
        case "tool_result":
          blocks.push({
            toolResult: {
              toolUseId: toolCallId(block),
              content: [{ text: block.content }],
              status: block.isError === true ? "error" : "success",
            },
          });
          break;
        case "image_base64": {
          const format = bedrockImageFormat(block.mediaType);
          if (format === undefined) {
            return unsupported("aws_bedrock", `image_media_type:${block.mediaType}`);
          }
          blocks.push({ image: { format, source: { bytes: block.data } } });
          break;
        }
        case "image_url":
          // Bedrock accepts inline bytes or an S3 location, never a bare URL.
          return unsupported("aws_bedrock", "image_url");
        case "audio":
          return unsupported("aws_bedrock", "audio_input");
      }
    }
    messages.push({ role, content: blocks });
  }
  const out: JSONObject = { messages };
  if (system.length > 0) out.system = system;
  if (req.maxTokens !== undefined || req.temperature !== undefined) {
    const inference: JSONObject = {};
    if (req.maxTokens !== undefined) inference.maxTokens = req.maxTokens;
    if (req.temperature !== undefined) inference.temperature = req.temperature;
    out.inferenceConfig = inference;
  }
  if (req.tools && req.tools.length > 0) {
    const toolConfig: JSONObject = {
      tools: req.tools.map((tool) => ({
        toolSpec: {
          name: tool.name,
          description: tool.description,
          inputSchema: { json: tool.parameters },
        },
      })),
    };
    if (req.toolChoice !== undefined) {
      // Bedrock models decide whether they honour a forced choice; the wire shape
      // is a one-member union. `none` has no Bedrock equivalent, so it must fail
      // loudly rather than silently degrade.
      if (req.toolChoice === "auto") toolConfig.toolChoice = { auto: {} };
      else if (req.toolChoice === "required") toolConfig.toolChoice = { any: {} };
      else if (req.toolChoice === "none") return unsupported("aws_bedrock", "tool_choice_none");
      else toolConfig.toolChoice = { tool: { name: req.toolChoice.tool } };
    }
    out.toolConfig = toolConfig;
  }
  if ((req.thinking?.budgetTokens ?? 0) > 0) {
    out.additionalModelRequestFields = {
      thinking: { type: "enabled", budget_tokens: req.thinking?.budgetTokens },
    };
  }
  return out;
}

function toOllamaChat(req: LmpRequest): JSONObject {
  if (req.responseFormat?.type === "json_schema") {
    return unsupported("ollama", "json_schema_strict");
  }
  // Ollama has no tool_choice field; only "declare the tools or not" is expressible.
  let declareTools = req.tools !== undefined && req.tools.length > 0;
  if (req.toolChoice !== undefined) {
    if (req.toolChoice === "required" || typeof req.toolChoice === "object") {
      return unsupported("ollama", "tool_choice");
    }
    if (req.toolChoice === "none") declareTools = false;
  }
  const messages: JSONObject[] = [];
  // Ollama correlates tool output by function name rather than by call id, so the
  // mapping is built while walking the assistant turn.
  const callNames = new Map<string, string>();
  for (const message of req.messages) {
    let text = "";
    let thinking: string | undefined;
    const images: string[] = [];
    const toolCalls: JSONObject[] = [];
    const toolResults: Array<{ name: string; content: string }> = [];
    for (const block of message.content) {
      switch (block.type) {
        case "text":
          text += block.text;
          break;
        case "thinking":
          thinking = thinking === undefined ? block.thinking : thinking + block.thinking;
          break;
        case "image_base64":
          images.push(block.data);
          break;
        case "image_url":
          images.push(block.url);
          break;
        case "tool_call":
          callNames.set(block.id, block.name);
          toolCalls.push({
            type: "function",
            function: { index: toolCalls.length, name: block.name, arguments: block.arguments },
          });
          break;
        case "tool_result":
          toolResults.push({ name: callNames.get(toolCallId(block)) ?? "", content: block.content });
          break;
        case "audio":
          return unsupported("ollama", "audio_input");
      }
    }
    if (text !== "" || thinking !== undefined || images.length > 0 || toolCalls.length > 0) {
      const entry: JSONObject = { role: message.role, content: text };
      if (thinking !== undefined) entry.thinking = thinking;
      if (images.length > 0) entry.images = images;
      if (toolCalls.length > 0) entry.tool_calls = toolCalls;
      messages.push(entry);
    }
    for (const result of toolResults) {
      const entry: JSONObject = { role: "tool", content: result.content };
      if (result.name !== "") entry.tool_name = result.name;
      messages.push(entry);
    }
  }
  const out: JSONObject = { model: req.model, messages, stream: false };
  if (req.responseFormat?.type === "json_object") out.format = "json";
  if (declareTools && req.tools !== undefined) out.tools = openAITools(req.tools);
  if (req.thinking !== undefined) {
    const effort = req.thinking.effort;
    if (effort === undefined) {
      out.think = true;
    } else if (["minimal", "low", "medium", "high"].includes(effort)) {
      out.think = effort;
    } else {
      return unsupported("ollama", "thinking_effort");
    }
  }
  const options: JSONObject = {};
  if (req.temperature !== undefined) options.temperature = req.temperature;
  if (req.maxTokens !== undefined) options.num_predict = req.maxTokens;
  if (Object.keys(options).length > 0) out.options = options;
  return out;
}

function toCohereChat(req: LmpRequest): JSONObject {
  const messages: JSONObject[] = [];
  for (const message of req.messages) {
    let text = "";
    let thinking = "";
    const images: JSONObject[] = [];
    const toolCalls: JSONObject[] = [];
    const toolResults: Array<{ id: string; content: string }> = [];
    for (const block of message.content) {
      switch (block.type) {
        case "text":
          text += block.text;
          break;
        case "thinking":
          thinking += block.thinking;
          break;
        case "image_url":
          images.push({ type: "image_url", image_url: { url: block.url } });
          break;
        case "image_base64":
          return unsupported("cohere", "image_base64");
        case "audio":
          return unsupported("cohere", "audio_input");
        case "tool_call":
          toolCalls.push({
            id: block.id,
            type: "function",
            function: { name: block.name, arguments: argumentsString(block.arguments) },
          });
          break;
        case "tool_result":
          toolResults.push({ id: toolCallId(block), content: block.content });
          break;
      }
    }
    if (message.role === "assistant") {
      // Cohere keeps reasoning and answer text as separate content blocks on the
      // assistant turn instead of a plain string.
      const blocks: JSONObject[] = [];
      if (thinking !== "") blocks.push({ type: "thinking", thinking });
      if (text !== "") blocks.push({ type: "text", text });
      const entry: JSONObject = { role: "assistant" };
      if (blocks.length > 0) entry.content = blocks;
      else entry.content = "";
      if (toolCalls.length > 0) entry.tool_calls = toolCalls;
      messages.push(entry);
    } else if (images.length > 0) {
      const blocks: JSONObject[] = [];
      if (text !== "") blocks.push({ type: "text", text });
      blocks.push(...images);
      messages.push({ role: message.role, content: blocks });
    } else if (text !== "") {
      messages.push({ role: message.role, content: text });
    }
    for (const result of toolResults) {
      messages.push({
        role: "tool",
        tool_call_id: result.id,
        content: [{ type: "document", document: { data: result.content } }],
      });
    }
  }
  const out: JSONObject = { model: req.model, messages };
  if (req.maxTokens !== undefined) out.max_tokens = req.maxTokens;
  if (req.temperature !== undefined) out.temperature = req.temperature;
  if (req.tools && req.tools.length > 0) out.tools = openAITools(req.tools);
  if (req.toolChoice !== undefined) {
    // Cohere only understands REQUIRED / NONE; its default is auto, so `auto` is
    // expressed by omitting the field rather than forcing NONE.
    if (req.toolChoice === "required") out.tool_choice = "REQUIRED";
    else if (req.toolChoice === "none") out.tool_choice = "NONE";
    else if (typeof req.toolChoice === "object") {
      return unsupported("cohere", "tool_choice_tool");
    }
    if (out.tool_choice !== undefined && (!req.tools || req.tools.length === 0)) {
      return unsupported("cohere", "tool_choice_without_tools");
    }
  }
  const format = req.responseFormat;
  if (format?.type === "json_object") out.response_format = { type: "json_object" };
  else if (format?.type === "json_schema") {
    out.response_format = { type: "json_object", json_schema: format.schema };
  }
  if (req.thinking !== undefined) {
    const thinking: JSONObject = { type: "enabled" };
    if (req.thinking.budgetTokens !== undefined) thinking.token_budget = req.thinking.budgetTokens;
    out.thinking = thinking;
  }
  return out;
}

/**
 * Normalizes one complete native response into an LMP response.
 *
 * Mirrors `normalize_response` in the Rust core: the provider-specific shapes
 * collapse into the same ordered content blocks so a caller can replay them.
 */
export function normalizeResponse(protocol: NativeProtocol, raw: JSONObject): LmpResponse {
  switch (protocol) {
    case "openai_chat_completions":
    case "azure_openai":
      return fromOpenAIChat(raw);
    case "openai_responses":
      return fromOpenAIResponses(raw);
    case "anthropic_messages":
      return fromAnthropicMessages(raw);
    case "google_gemini":
    case "google_vertex_ai":
      return fromGoogleGemini(raw);
    case "aws_bedrock":
      return fromAWSBedrock(raw);
    case "ollama":
      return fromOllamaChat(raw);
    case "cohere":
      return fromCohereChat(raw);
  }
}

function fromOpenAIChat(raw: JSONObject): LmpResponse {
  const choices = arrAt(raw, "choices");
  if (choices.length === 0) throw new Error("missing choices[0]");
  const choice = objAt(choices[0]) ?? {};
  const message = objAt(choice, "message") ?? {};
  const content: LmpContentBlock[] = [];
  const reasoning = strAt(message, "reasoning_content");
  if (reasoning !== "") content.push({ type: "thinking", thinking: reasoning });
  const text = strAt(message, "content");
  if (text !== "") content.push({ type: "text", text });
  for (const rawCall of arrAt(message, "tool_calls")) {
    const call = objAt(rawCall) ?? {};
    content.push({
      type: "tool_call",
      id: strAt(call, "id"),
      name: strAt(call, "function", "name"),
      arguments: argumentsObject(lookup(call, "function", "arguments")),
    });
  }
  return {
    id: optionalString(raw.id),
    model: optionalString(raw.model),
    content,
    finishReason: optionalString(choice.finish_reason),
    usage: parseOpenAIUsage(objAt(raw, "usage")),
    raw,
  };
}

function fromOpenAIResponses(raw: JSONObject): LmpResponse {
  const content: LmpContentBlock[] = [];
  for (const rawItem of arrAt(raw, "output")) {
    const item = objAt(rawItem) ?? {};
    switch (strAt(item, "type")) {
      case "reasoning": {
        const summary: string[] = [];
        for (const rawPart of arrAt(item, "summary")) {
          const text = strAt(rawPart, "text");
          if (text !== "") summary.push(text);
        }
        const thinking = summary.join("\n");
        // encrypted_content is the opaque token that preserves reasoning state
        // across a stateless (store=false) replay.
        const signature = strAt(item, "encrypted_content");
        if (thinking !== "" || signature !== "") {
          content.push(
            signature === ""
              ? { type: "thinking", thinking }
              : { type: "thinking", thinking, signature },
          );
        }
        break;
      }
      case "message":
        for (const rawPart of arrAt(item, "content")) {
          const text = strAt(rawPart, "text");
          if (text !== "") content.push({ type: "text", text });
        }
        break;
      case "function_call":
        content.push({
          type: "tool_call",
          id: strAt(item, "call_id"),
          name: strAt(item, "name"),
          arguments: argumentsObject(lookup(item, "arguments")),
        });
        break;
    }
  }
  const usage = objAt(raw, "usage");
  const inputTokens = intOr(usage, "input_tokens");
  const outputTokens = intOr(usage, "output_tokens");
  const total = intOr(usage, "total_tokens");
  return {
    id: optionalString(raw.id),
    model: optionalString(raw.model),
    content,
    finishReason: optionalString(raw.status),
    usage: usageWith({
      inputTokens,
      outputTokens,
      totalTokens: total === 0 ? inputTokens + outputTokens : total,
      reasoningTokens: optInt(usage, "output_tokens_details", "reasoning_tokens"),
      cacheReadTokens: optInt(usage, "input_tokens_details", "cached_tokens"),
    }),
    raw,
  };
}

function fromAnthropicMessages(raw: JSONObject): LmpResponse {
  const content: LmpContentBlock[] = [];
  for (const rawBlock of arrAt(raw, "content")) {
    const block = objAt(rawBlock) ?? {};
    switch (strAt(block, "type")) {
      case "thinking": {
        const thinking = strAt(block, "thinking");
        const signature = strAt(block, "signature");
        content.push(
          signature === "" ? { type: "thinking", thinking } : { type: "thinking", thinking, signature },
        );
        break;
      }
      case "text":
        content.push({ type: "text", text: strAt(block, "text") });
        break;
      case "tool_use":
        content.push({
          type: "tool_call",
          id: strAt(block, "id"),
          name: strAt(block, "name"),
          arguments: argumentsObject(lookup(block, "input")),
        });
        break;
    }
  }
  const usage = objAt(raw, "usage");
  const inputTokens = intOr(usage, "input_tokens");
  const outputTokens = intOr(usage, "output_tokens");
  return {
    id: optionalString(raw.id),
    model: optionalString(raw.model),
    content,
    finishReason: optionalString(raw.stop_reason),
    usage: usageWith({
      inputTokens,
      outputTokens,
      totalTokens: intOr(usage, "total_tokens") || inputTokens + outputTokens,
      cacheReadTokens: optInt(usage, "cache_read_input_tokens"),
      cacheWriteTokens: optInt(usage, "cache_creation_input_tokens"),
    }),
    raw,
  };
}

function parseOpenAIUsage(usage: JSONObject | undefined): LmpUsage {
  return usageWith({
    inputTokens: intOr(usage, "prompt_tokens"),
    outputTokens: intOr(usage, "completion_tokens"),
    totalTokens: intOr(usage, "total_tokens"),
    reasoningTokens: optInt(usage, "completion_tokens_details", "reasoning_tokens"),
    cacheReadTokens: optInt(usage, "prompt_tokens_details", "cached_tokens"),
  });
}

/** Drops undefined optional counters so a JSON round-trip stays stable. */
function usageWith(usage: LmpUsage): LmpUsage {
  const out: { -readonly [K in keyof LmpUsage]: LmpUsage[K] } = {
    inputTokens: usage.inputTokens,
    outputTokens: usage.outputTokens,
    totalTokens: usage.totalTokens,
  };
  if (usage.reasoningTokens !== undefined) out.reasoningTokens = usage.reasoningTokens;
  if (usage.cacheReadTokens !== undefined) out.cacheReadTokens = usage.cacheReadTokens;
  if (usage.cacheWriteTokens !== undefined) out.cacheWriteTokens = usage.cacheWriteTokens;
  return out;
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function fromGoogleGemini(raw: JSONObject): LmpResponse {
  const candidates = arrAt(raw, "candidates");
  if (candidates.length === 0) throw new Error("missing candidates[0]");
  const candidate = objAt(candidates[0]) ?? {};
  const content: LmpContentBlock[] = [];
  let index = 0;
  for (const rawPart of arrAt(candidate, "content", "parts")) {
    const part = objAt(rawPart) ?? {};
    const signature = strAt(part, "thoughtSignature");
    const text = lookup(part, "text");
    if (typeof text === "string") {
      if (lookup(part, "thought") === true) {
        content.push(signature === "" ? { type: "thinking", thinking: text } : { type: "thinking", thinking: text, signature });
      } else {
        content.push(signature === "" ? { type: "text", text } : { type: "text", text, signature });
      }
    } else {
      const call = objAt(part, "functionCall");
      if (call !== undefined) {
        const name = strAt(call, "name");
        if (name === "") throw new Error("functionCall missing name");
        // Google omits the call id for a single-call turn; a local id keeps the
        // matching tool_result replayable without inventing a wire id.
        const wireId = strAt(call, "id");
        const id = wireId === "" ? `gemini-part:${index}:${name}` : wireId;
        const args = argumentsObject(lookup(call, "args"));
        content.push(
          signature === ""
            ? { type: "tool_call", id, name, arguments: args }
            : { type: "tool_call", id, name, arguments: args, signature },
        );
      } else if (signature !== "") {
        throw new Error("Gemini returned a signature on an unsupported part");
      }
    }
    index += 1;
  }
  const usage = objAt(raw, "usageMetadata");
  const inputTokens = intOr(usage, "promptTokenCount");
  const outputTokens = intOr(usage, "candidatesTokenCount");
  return {
    content,
    finishReason: optionalString(candidate.finishReason),
    usage: usageWith({
      inputTokens,
      outputTokens,
      totalTokens: intOr(usage, "totalTokenCount") || inputTokens + outputTokens,
      reasoningTokens: optInt(usage, "thoughtsTokenCount"),
      cacheReadTokens: optInt(usage, "cachedContentTokenCount"),
    }),
    raw,
  };
}

function fromAWSBedrock(raw: JSONObject): LmpResponse {
  const content: LmpContentBlock[] = [];
  for (const rawBlock of arrAt(raw, "output", "message", "content")) {
    const block = objAt(rawBlock) ?? {};
    const text = lookup(block, "text");
    const reasoning = objAt(block, "reasoningContent", "reasoningText");
    const tool = objAt(block, "toolUse");
    if (typeof text === "string" && text !== "") {
      content.push({ type: "text", text });
    } else if (reasoning !== undefined) {
      const thinking = strAt(reasoning, "text");
      const signature = strAt(reasoning, "signature");
      content.push(signature === "" ? { type: "thinking", thinking } : { type: "thinking", thinking, signature });
    } else if (tool !== undefined) {
      content.push({
        type: "tool_call",
        id: strAt(tool, "toolUseId"),
        name: strAt(tool, "name"),
        arguments: argumentsObject(lookup(tool, "input")),
      });
    }
  }
  const usage = objAt(raw, "usage");
  const inputTokens = intOr(usage, "inputTokens");
  const outputTokens = intOr(usage, "outputTokens");
  return {
    content,
    finishReason: optionalString(raw.stopReason),
    usage: usageWith({
      inputTokens,
      outputTokens,
      totalTokens: intOr(usage, "totalTokens") || inputTokens + outputTokens,
      cacheReadTokens: optInt(usage, "cacheReadInputTokens"),
      cacheWriteTokens: optInt(usage, "cacheWriteInputTokens"),
    }),
    raw,
  };
}

function fromOllamaChat(raw: JSONObject): LmpResponse {
  const message = objAt(raw, "message") ?? {};
  const content: LmpContentBlock[] = [];
  const thinking = strAt(message, "thinking");
  if (thinking !== "") content.push({ type: "thinking", thinking });
  const text = strAt(message, "content");
  if (text !== "") content.push({ type: "text", text });
  let position = 0;
  for (const rawCall of arrAt(message, "tool_calls")) {
    const call = objAt(rawCall) ?? {};
    const name = strAt(call, "function", "name");
    // Ollama correlates tool output by function name, so the synthesized id is a
    // local handle only.
    const index = numAt(call, "function", "index") ?? position;
    content.push({
      type: "tool_call",
      id: `ollama-call:${Math.trunc(index)}:${name}`,
      name,
      arguments: argumentsObject(lookup(call, "function", "arguments")),
    });
    position += 1;
  }
  const inputTokens = intOr(raw, "prompt_eval_count");
  const outputTokens = intOr(raw, "eval_count");
  return {
    model: optionalString(raw.model),
    content,
    finishReason: optionalString(raw.done_reason),
    usage: usageWith({ inputTokens, outputTokens, totalTokens: inputTokens + outputTokens }),
    raw,
  };
}

function fromCohereChat(raw: JSONObject): LmpResponse {
  const message = objAt(raw, "message") ?? {};
  const content: LmpContentBlock[] = [];
  for (const rawBlock of arrAt(message, "content")) {
    const block = objAt(rawBlock) ?? {};
    switch (strAt(block, "type")) {
      case "thinking": {
        const thinking = strAt(block, "thinking");
        if (thinking !== "") content.push({ type: "thinking", thinking });
        break;
      }
      case "text": {
        const text = strAt(block, "text");
        if (text !== "") content.push({ type: "text", text });
        break;
      }
    }
  }
  for (const rawCall of arrAt(message, "tool_calls")) {
    const call = objAt(rawCall) ?? {};
    content.push({
      type: "tool_call",
      id: strAt(call, "id"),
      name: strAt(call, "function", "name"),
      arguments: argumentsObject(lookup(call, "function", "arguments")),
    });
  }
  const tokens = objAt(raw, "usage", "tokens");
  const inputTokens = intOr(tokens, "input_tokens");
  const outputTokens = intOr(tokens, "output_tokens");
  return {
    id: optionalString(raw.id),
    content,
    finishReason: optionalString(raw.finish_reason),
    usage: usageWith({ inputTokens, outputTokens, totalTokens: inputTokens + outputTokens }),
    raw,
  };
}

/**
 * Converts provider streaming frames into LMP stream events.
 *
 * It is stateful on purpose: tool arguments arrive as fragments that must be
 * attributed to the right call, thinking signatures may land in a later frame
 * than the text they belong to, and message_start / message_end have to be
 * emitted exactly once regardless of the upstream framing.
 */
export class StreamNormalizer {
  private readonly protocol: NativeProtocol;
  private started = false;
  private ended = false;
  private usage: LmpUsage = { inputTokens: 0, outputTokens: 0, totalTokens: 0 };
  private finishReason: string | undefined;
  /** Anthropic content block index -> tool call id/name. */
  private readonly anthropicTools = new Map<number, { id: string; name: string }>();
  /** Gemini sends one part per frame, so a monotonic counter identifies calls. */
  private geminiToolCalls = 0;

  constructor(protocol: NativeProtocol) {
    this.protocol = protocol;
  }

  /** Reports whether a terminal message_end has been produced. */
  get finished(): boolean {
    return this.ended;
  }

  /** Normalizes one SSE/NDJSON frame into zero or more LMP stream events. */
  push(event: SseEvent): LmpStreamEvent[] {
    const out: LmpStreamEvent[] = [];
    const trimmed = event.data.trim();
    if (trimmed === "" || trimmed === "[DONE]") return out;
    const value = asRecord(JSON.parse(trimmed)) ?? {};
    switch (this.protocol) {
      case "openai_chat_completions":
      case "azure_openai":
        this.pushOpenAIChat(value, out);
        break;
      case "openai_responses":
        this.pushOpenAIResponses(event.event, value, out);
        break;
      case "anthropic_messages":
        this.pushAnthropic(event.event, value, out);
        break;
      case "google_gemini":
      case "google_vertex_ai":
        this.pushGemini(value, out);
        break;
      case "aws_bedrock":
        this.pushBedrock(value, out);
        break;
      case "ollama":
        this.pushOllama(value, out);
        break;
      case "cohere":
        this.pushCohere(event.event, value, out);
        break;
    }
    return out;
  }

  /** Emits message_end for streams that end without an explicit terminal frame. */
  finish(): LmpStreamEvent[] {
    if (this.ended) return [];
    this.ended = true;
    const event: LmpStreamEvent =
      this.finishReason === undefined
        ? { type: "message_end" }
        : { type: "message_end", finishReason: this.finishReason };
    return [event];
  }

  private markStarted(id: string, out: LmpStreamEvent[]): void {
    if (this.started) return;
    this.started = true;
    out.push(id === "" ? { type: "message_start" } : { type: "message_start", id });
  }

  private markEnded(finishReason: string, out: LmpStreamEvent[]): void {
    if (finishReason !== "") this.finishReason = finishReason;
    if (this.ended) return;
    this.ended = true;
    const event: LmpStreamEvent =
      finishReason === "" ? { type: "message_end" } : { type: "message_end", finishReason };
    out.push(event);
  }

  /** Keeps only changes: providers repeat identical counters across frames. */
  private emitUsage(next: LmpUsage, out: LmpStreamEvent[]): void {
    const changed = !sameUsage(this.usage, next);
    this.usage = next;
    if (changed) out.push({ type: "usage", usage: next });
  }

  private pushOpenAIChat(value: JSONObject, out: LmpStreamEvent[]): void {
    const choices = arrAt(value, "choices");
    if (choices.length > 0) {
      const choice = objAt(choices[0]) ?? {};
      const delta = objAt(choice, "delta");
      if (delta !== undefined) {
        const reasoning = strAt(delta, "reasoning_content");
        if (reasoning !== "") out.push({ type: "thinking_delta", delta: reasoning });
        const text = strAt(delta, "content");
        if (text !== "") out.push({ type: "text_delta", delta: text });
        arrAt(delta, "tool_calls").forEach((rawCall, position) => {
          const call = objAt(rawCall) ?? {};
          const index = numAt(call, "index") ?? position;
          const id = strAt(call, "id");
          const name = strAt(call, "function", "name");
          const argumentsDelta = strAt(call, "function", "arguments");
          const event: LmpStreamEvent = { type: "tool_call_delta", index: Math.trunc(index) };
          if (id !== "") event.id = id;
          if (name !== "") event.name = name;
          if (argumentsDelta !== "") event.argumentsDelta = argumentsDelta;
          out.push(event);
        });
      }
      const reason = strAt(choice, "finish_reason");
      if (reason !== "") this.markEnded(reason, out);
    }
    const usage = objAt(value, "usage");
    if (usage !== undefined) this.emitUsage(parseOpenAIUsage(usage), out);
  }

  private pushOpenAIResponses(eventName: string | null, value: JSONObject, out: LmpStreamEvent[]): void {
    const kind = eventName === null || eventName === "" ? strAt(value, "type") : eventName;
    switch (kind) {
      case "response.created":
        this.markStarted(strAt(value, "response", "id"), out);
        break;
      case "response.output_text.delta": {
        const delta = strAt(value, "delta");
        if (delta !== "") out.push({ type: "text_delta", delta });
        break;
      }
      case "response.reasoning_summary_text.delta":
      case "response.reasoning_text.delta": {
        const delta = strAt(value, "delta");
        if (delta !== "") out.push({ type: "thinking_delta", delta });
        break;
      }
      case "response.output_item.added": {
        const item = objAt(value, "item") ?? {};
        if (strAt(item, "type") !== "function_call") break;
        const index = Math.trunc(intOr(value, "output_index"));
        const callId = strAt(item, "call_id");
        const id = callId === "" ? strAt(item, "id") : callId;
        const name = strAt(item, "name");
        const event: LmpStreamEvent = { type: "tool_call_delta", index };
        if (id !== "") event.id = id;
        if (name !== "") event.name = name;
        out.push(event);
        break;
      }
      case "response.function_call_arguments.delta": {
        const delta = strAt(value, "delta");
        if (delta !== "") {
          out.push({ type: "tool_call_delta", index: Math.trunc(intOr(value, "output_index")), argumentsDelta: delta });
        }
        break;
      }
      case "response.completed": {
        const response = objAt(value, "response") ?? {};
        const usage = objAt(response, "usage");
        if (usage !== undefined) {
          const inputTokens = intOr(usage, "input_tokens");
          const outputTokens = intOr(usage, "output_tokens");
          const total = intOr(usage, "total_tokens");
          this.emitUsage(
            usageWith({
              inputTokens,
              outputTokens,
              totalTokens: total === 0 ? inputTokens + outputTokens : total,
              reasoningTokens: optInt(usage, "output_tokens_details", "reasoning_tokens"),
              cacheReadTokens: optInt(usage, "input_tokens_details", "cached_tokens"),
            }),
            out,
          );
        }
        this.markEnded(strAt(response, "status"), out);
        break;
      }
    }
  }

  private pushAnthropic(eventName: string | null, value: JSONObject, out: LmpStreamEvent[]): void {
    const kind = eventName === null || eventName === "" ? strAt(value, "type") : eventName;
    switch (kind) {
      case "message_start": {
        const message = objAt(value, "message") ?? {};
        this.markStarted(strAt(message, "id"), out);
        const usage = objAt(message, "usage");
        if (usage !== undefined) this.emitUsage(parseAnthropicUsage(usage, this.usage), out);
        break;
      }
      case "content_block_start": {
        const index = Math.trunc(intOr(value, "index"));
        const block = objAt(value, "content_block") ?? {};
        switch (strAt(block, "type")) {
          case "tool_use": {
            const id = strAt(block, "id");
            const name = strAt(block, "name");
            this.anthropicTools.set(index, { id, name });
            const event: LmpStreamEvent = { type: "tool_call_delta", index };
            if (id !== "") event.id = id;
            if (name !== "") event.name = name;
            out.push(event);
            break;
          }
          case "thinking": {
            // The signature may be announced up front; forward it as its own delta
            // so a replay can attach it to the reasoning text.
            const signature = strAt(block, "signature");
            if (signature !== "") out.push({ type: "thinking_delta", delta: "", signature });
            break;
          }
        }
        break;
      }
      case "content_block_delta": {
        const index = Math.trunc(intOr(value, "index"));
        const delta = objAt(value, "delta") ?? {};
        switch (strAt(delta, "type")) {
          case "thinking_delta": {
            const text = strAt(delta, "thinking");
            if (text !== "") out.push({ type: "thinking_delta", delta: text });
            break;
          }
          case "text_delta": {
            const text = strAt(delta, "text");
            if (text !== "") out.push({ type: "text_delta", delta: text });
            break;
          }
          case "input_json_delta": {
            const partial = strAt(delta, "partial_json");
            if (partial !== "") {
              const known = this.anthropicTools.get(index);
              const event: LmpStreamEvent = { type: "tool_call_delta", index, argumentsDelta: partial };
              if (known !== undefined) {
                if (known.id !== "") event.id = known.id;
                if (known.name !== "") event.name = known.name;
              }
              out.push(event);
            }
            break;
          }
          case "signature_delta": {
            const signature = strAt(delta, "signature");
            if (signature !== "") out.push({ type: "thinking_delta", delta: "", signature });
            break;
          }
        }
        break;
      }
      case "message_delta": {
        const usage = objAt(value, "usage");
        if (usage !== undefined) this.emitUsage(parseAnthropicUsage(usage, this.usage), out);
        // message_delta announces the stop reason; the terminal message_end waits
        // for message_stop.
        const reason = strAt(value, "delta", "stop_reason");
        if (reason !== "") this.finishReason = reason;
        break;
      }
      case "message_stop":
        this.markEnded(this.finishReason ?? "", out);
        break;
    }
  }

  private pushGemini(value: JSONObject, out: LmpStreamEvent[]): void {
    const candidates = arrAt(value, "candidates");
    if (candidates.length > 0) {
      const candidate = objAt(candidates[0]) ?? {};
      for (const rawPart of arrAt(candidate, "content", "parts")) {
        const part = objAt(rawPart) ?? {};
        const signature = strAt(part, "thoughtSignature");
        const text = lookup(part, "text");
        if (typeof text === "string") {
          const isThought = lookup(part, "thought") === true;
          if (isThought) {
            out.push(signature === "" ? { type: "thinking_delta", delta: text } : { type: "thinking_delta", delta: text, signature });
          } else {
            out.push(signature === "" ? { type: "text_delta", delta: text } : { type: "text_delta", delta: text, signature });
          }
        } else {
          const call = objAt(part, "functionCall");
          if (call !== undefined) {
            // Each frame carries one part, so the part offset cannot identify
            // parallel calls; a monotonic counter can.
            const index = this.geminiToolCalls;
            this.geminiToolCalls += 1;
            const args = lookup(call, "args");
            const argumentsDelta = args === undefined || args === null ? "" : JSON.stringify(args);
            const event: LmpStreamEvent = { type: "tool_call_delta", index };
            const id = strAt(call, "id");
            const name = strAt(call, "name");
            if (id !== "") event.id = id;
            if (name !== "") event.name = name;
            if (argumentsDelta !== "") event.argumentsDelta = argumentsDelta;
            if (signature !== "") event.signature = signature;
            out.push(event);
          }
        }
      }
      const reason = strAt(candidate, "finishReason");
      if (reason !== "") this.finishReason = reason;
    }
    const usage = objAt(value, "usageMetadata");
    if (usage !== undefined) {
      const inputTokens = intOr(usage, "promptTokenCount");
      const outputTokens = intOr(usage, "candidatesTokenCount");
      const total = intOr(usage, "totalTokenCount");
      this.emitUsage(
        usageWith({
          inputTokens,
          outputTokens,
          totalTokens: total === 0 ? inputTokens + outputTokens : total,
          reasoningTokens: optInt(usage, "thoughtsTokenCount"),
          cacheReadTokens: optInt(usage, "cachedContentTokenCount"),
        }),
        out,
      );
    }
  }

  private pushBedrock(value: JSONObject, out: LmpStreamEvent[]): void {
    const delta = objAt(value, "contentBlockDelta");
    if (delta !== undefined) {
      const index = Math.trunc(intOr(delta, "contentBlockIndex"));
      const payload = objAt(delta, "delta") ?? {};
      const text = lookup(payload, "text");
      if (typeof text === "string" && text !== "") out.push({ type: "text_delta", delta: text });
      const reasoning = objAt(payload, "reasoningContent");
      if (reasoning !== undefined) {
        const thinking = strAt(reasoning, "text");
        if (thinking !== "") out.push({ type: "thinking_delta", delta: thinking });
        const signature = strAt(reasoning, "signature");
        if (signature !== "") out.push({ type: "thinking_delta", delta: "", signature });
      }
      const input = lookup(payload, "toolUse", "input");
      if (typeof input === "string" && input !== "") {
        out.push({ type: "tool_call_delta", index, argumentsDelta: input });
      }
      return;
    }
    const start = objAt(value, "contentBlockStart", "start", "toolUse");
    if (start !== undefined) {
      const index = Math.trunc(intOr(value, "contentBlockStart", "contentBlockIndex"));
      const id = strAt(start, "toolUseId");
      const name = strAt(start, "name");
      const event: LmpStreamEvent = { type: "tool_call_delta", index };
      if (id !== "") event.id = id;
      if (name !== "") event.name = name;
      out.push(event);
      return;
    }
    const reason = strAt(value, "messageStop", "stopReason");
    if (reason !== "") {
      this.markEnded(reason, out);
      return;
    }
    const usage = objAt(value, "metadata", "usage");
    if (usage !== undefined) {
      const inputTokens = intOr(usage, "inputTokens");
      const outputTokens = intOr(usage, "outputTokens");
      const total = intOr(usage, "totalTokens");
      this.emitUsage(
        usageWith({
          inputTokens,
          outputTokens,
          totalTokens: total === 0 ? inputTokens + outputTokens : total,
          cacheReadTokens: optInt(usage, "cacheReadInputTokens"),
          cacheWriteTokens: optInt(usage, "cacheWriteInputTokens"),
        }),
        out,
      );
    }
  }

  private pushOllama(value: JSONObject, out: LmpStreamEvent[]): void {
    const message = objAt(value, "message") ?? {};
    const thinking = strAt(message, "thinking");
    if (thinking !== "") out.push({ type: "thinking_delta", delta: thinking });
    const text = strAt(message, "content");
    if (text !== "") out.push({ type: "text_delta", delta: text });
    arrAt(message, "tool_calls").forEach((rawCall, position) => {
      const call = objAt(rawCall) ?? {};
      const name = strAt(call, "function", "name");
      const index = Math.trunc(numAt(call, "function", "index") ?? position);
      const args = lookup(call, "function", "arguments");
      const event: LmpStreamEvent = {
        type: "tool_call_delta",
        index,
        id: `ollama-call:${index}:${name}`,
        argumentsDelta: JSON.stringify(args ?? {}),
      };
      if (name !== "") event.name = name;
      out.push(event);
    });
    if (lookup(value, "done") === true) {
      const inputTokens = intOr(value, "prompt_eval_count");
      const outputTokens = intOr(value, "eval_count");
      this.emitUsage(usageWith({ inputTokens, outputTokens, totalTokens: inputTokens + outputTokens }), out);
      this.markEnded(strAt(value, "done_reason"), out);
    }
  }

  private pushCohere(eventName: string | null, value: JSONObject, out: LmpStreamEvent[]): void {
    const kind = eventName === null || eventName === "" ? strAt(value, "type") : eventName;
    switch (kind) {
      case "message-start":
        this.markStarted(strAt(value, "id"), out);
        break;
      case "tool-call-start": {
        // Cohere announces each call, then streams its arguments in fragments.
        const index = Math.trunc(intOr(value, "index"));
        const raw = lookup(value, "delta", "message", "tool_calls");
        const call = Array.isArray(raw) ? asRecord(raw[0]) ?? {} : asRecord(raw) ?? {};
        const id = strAt(call, "id");
        const name = strAt(call, "function", "name");
        const event: LmpStreamEvent = { type: "tool_call_delta", index };
        if (id !== "") event.id = id;
        if (name !== "") event.name = name;
        out.push(event);
        break;
      }
      case "tool-call-delta": {
        const delta = strAt(value, "delta", "message", "tool_calls", "function", "arguments");
        if (delta !== "") {
          out.push({ type: "tool_call_delta", index: Math.trunc(intOr(value, "index")), argumentsDelta: delta });
        }
        break;
      }
      case "content-delta": {
        const text = strAt(value, "delta", "message", "content", "text");
        if (text !== "") out.push({ type: "text_delta", delta: text });
        const thinking = strAt(value, "delta", "message", "content", "thinking");
        if (thinking !== "") out.push({ type: "thinking_delta", delta: thinking });
        break;
      }
      case "message-end": {
        const tokens = objAt(value, "delta", "usage", "tokens");
        if (tokens !== undefined) {
          const inputTokens = intOr(tokens, "input_tokens");
          const outputTokens = intOr(tokens, "output_tokens");
          this.emitUsage(usageWith({ inputTokens, outputTokens, totalTokens: inputTokens + outputTokens }), out);
        }
        this.markEnded(strAt(value, "delta", "finish_reason"), out);
        break;
      }
    }
  }
}

/** Buffers a whole stream through one normalizer. */
export function normalizeStreamEvents(
  protocol: NativeProtocol,
  events: readonly SseEvent[],
): LmpStreamEvent[] {
  const normalizer = new StreamNormalizer(protocol);
  const out: LmpStreamEvent[] = [];
  for (const event of events) out.push(...normalizer.push(event));
  out.push(...normalizer.finish());
  return out;
}

/** Normalizes one frame without cross-frame state. Prefer StreamNormalizer. */
export function normalizeStreamEvent(
  protocol: NativeProtocol,
  event: SseEvent,
): LmpStreamEvent[] {
  return new StreamNormalizer(protocol).push(event);
}

/**
 * Merges the cache counters of an Anthropic usage delta with the totals already
 * seen, because stream deltas only carry the changed counters.
 */
function parseAnthropicUsage(usage: JSONObject, previous: LmpUsage): LmpUsage {
  const inputTokens = numAt(usage, "input_tokens") ?? previous.inputTokens;
  const outputTokens = numAt(usage, "output_tokens") ?? previous.outputTokens;
  const total = numAt(usage, "total_tokens") ?? inputTokens + outputTokens;
  const cacheRead = optInt(usage, "cache_read_input_tokens") ?? previous.cacheReadTokens;
  const cacheWrite = optInt(usage, "cache_creation_input_tokens") ?? previous.cacheWriteTokens;
  return usageWith({
    inputTokens,
    outputTokens,
    totalTokens: total,
    cacheReadTokens: cacheRead,
    cacheWriteTokens: cacheWrite,
  });
}

function sameUsage(a: LmpUsage, b: LmpUsage): boolean {
  return (
    a.inputTokens === b.inputTokens &&
    a.outputTokens === b.outputTokens &&
    a.totalTokens === b.totalTokens &&
    a.reasoningTokens === b.reasoningTokens &&
    a.cacheReadTokens === b.cacheReadTokens &&
    a.cacheWriteTokens === b.cacheWriteTokens
  );
}
