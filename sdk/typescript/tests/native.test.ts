import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  NativeClient,
  NativeHttpStatusError,
  ProviderRegistry,
} from "../src/native/index.ts";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

function readFixture(name: string): string {
  return fs.readFileSync(path.join(rootDir, "conformance/native", name), "utf8");
}

test("native client supports all 4 protocols, SSE streaming, and registry dispatch", async () => {
  let mode = "chat_json";
  let lastPath = "";
  let lastAuth = "";

  const server = http.createServer((req, res) => {
    lastPath = req.url ?? "";
    lastAuth = String(
      req.headers.authorization ?? req.headers["x-api-key"] ?? req.headers["x-goog-api-key"] ?? ""
    );
    req.resume();
    req.on("end", () => {
      if (mode === "429") {
        res.writeHead(429, { "Content-Type": "application/json" });
        res.end(readFixture("error_429.json"));
        return;
      }
      const isSse = mode.endsWith("_sse");
      res.writeHead(200, {
        "Content-Type": isSse ? "text/event-stream" : "application/json",
      });
      const map: Record<string, string> = {
        chat_json: "openai_chat_response.json",
        chat_sse: "openai_chat_stream.sse",
        resp_json: "openai_responses_response.json",
        resp_sse: "openai_responses_stream.sse",
        ant_json: "anthropic_messages_response.json",
        ant_sse: "anthropic_messages_stream.sse",
        gem_json: "google_gemini_response.json",
        gem_sse: "google_gemini_stream.sse",
      };
      res.end(readFixture(map[mode]));
    });
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const addr = server.address() as { port: number };
  const base = `http://127.0.0.1:${addr.port}`;

  try {
    const client = new NativeClient();

    mode = "chat_json";
    const chat = await client.openaiChatCompletions(
      { baseURL: `${base}/v1`, apiKey: "sk-1" },
      { model: "gpt-5-mini", messages: [] }
    );
    assert.equal(lastPath, "/v1/chat/completions");
    assert.equal(lastAuth, "Bearer sk-1");
    assert.equal(chat.id, "chatcmpl-mock-1");

    mode = "chat_sse";
    const chatEvents = [];
    for await (const ev of client.streamOpenaiChatCompletions(
      { baseURL: `${base}/v1`, apiKey: "sk-1" },
      { model: "gpt-5-mini", messages: [] }
    )) {
      chatEvents.push(ev);
    }
    assert.equal(chatEvents.length, 3);
    assert.equal(chatEvents[2].data, "[DONE]");

    mode = "resp_json";
    const resp = await client.openaiResponses(
      { baseURL: `${base}/v1`, apiKey: "sk-2" },
      { model: "gpt-5", input: "hi" }
    );
    assert.equal(lastPath, "/v1/responses");
    assert.equal(resp.id, "resp-mock-1");

    mode = "resp_sse";
    const respEvents = [];
    for await (const ev of client.streamOpenaiResponses(
      { baseURL: `${base}/v1`, apiKey: "sk-2" },
      { model: "gpt-5", input: "hi" }
    )) {
      respEvents.push(ev);
    }
    assert.equal(respEvents.length, 4);
    assert.equal(respEvents[1].event, "response.output_text.delta");

    mode = "ant_json";
    const ant = await client.anthropicMessages(
      { baseURL: base, apiKey: "sk-ant" },
      { model: "claude-sonnet-4-5", max_tokens: 128, messages: [] }
    );
    assert.equal(lastPath, "/v1/messages");
    assert.equal(lastAuth, "sk-ant");
    assert.equal(ant.id, "msg_mock_1");

    mode = "ant_sse";
    const antEvents = [];
    for await (const ev of client.streamAnthropicMessages(
      { baseURL: base, apiKey: "sk-ant" },
      { model: "claude-sonnet-4-5", max_tokens: 128, messages: [] }
    )) {
      antEvents.push(ev);
    }
    assert.equal(antEvents.length, 4);
    assert.ok(antEvents[1].data.includes("thinking_delta"));

    mode = "gem_json";
    await client.googleGeminiGenerate(
      { baseURL: base, apiKey: "gem-1" },
      "gemini-2.5-pro",
      { contents: [] }
    );
    assert.equal(lastPath, "/v1beta/models/gemini-2.5-pro:generateContent");
    assert.equal(lastAuth, "gem-1");

    mode = "gem_sse";
    const gemEvents = [];
    for await (const ev of client.streamGoogleGeminiGenerate(
      { baseURL: base, apiKey: "gem-1" },
      "gemini-2.5-pro",
      { contents: [] }
    )) {
      gemEvents.push(ev);
    }
    assert.equal(gemEvents.length, 2);
    assert.equal(lastPath, "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse");

    const registry = ProviderRegistry.fromJSON(
      fs.readFileSync(path.join(rootDir, "registry/registry.json"), "utf8")
    );
    assert.ok(registry.size >= 300);
    mode = "429";
    await assert.rejects(
      () =>
        client.sendRegisteredJSON(registry, "deepseek", "openai_chat_completions", {
          modelID: "deepseek-reasoner",
          apiKey: "sk-ds",
          endpointOverride: base,
          payload: { model: "deepseek-reasoner" },
        }),
      (err: unknown) =>
        err instanceof NativeHttpStatusError &&
        err.status === 429 &&
        err.body.includes("rate_limit_exceeded")
    );
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
