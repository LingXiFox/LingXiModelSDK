import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import type { NativeProtocol } from "../src/native/index.ts";
import { LmpUnsupportedFeatureError, UnifiedClient } from "../src/unified/index.ts";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");

function readNativeFixture(name: string): string {
  return fs.readFileSync(path.join(rootDir, "conformance/native", name), "utf8");
}

test("unified LMP adapters normalize all 4 protocols and reject unsupported features", async () => {
  const exp = JSON.parse(
    fs.readFileSync(path.join(rootDir, "conformance/lmp/expectations.json"), "utf8")
  );
  let currentBody = "";
  let currentCT = "application/json";

  const server = http.createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      res.writeHead(200, { "Content-Type": currentCT });
      res.end(currentBody);
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${(server.address() as { port: number }).port}`;

  try {
    const client = new UnifiedClient();
    const cases: Array<[NativeProtocol, string, string, string]> = [
      ["openai_chat_completions", "openai_chat_response.json", "openai_chat_stream.sse", "gpt-5-mini"],
      ["openai_responses", "openai_responses_response.json", "openai_responses_stream.sse", "gpt-5"],
      ["anthropic_messages", "anthropic_messages_response.json", "anthropic_messages_stream.sse", "claude-sonnet-4-5"],
      ["google_gemini", "google_gemini_response.json", "google_gemini_stream.sse", "gemini-2.5-pro"],
    ];

    for (const [proto, respFile, sseFile, model] of cases) {
      currentCT = "application/json";
      currentBody = readNativeFixture(respFile);
      const resp = await client.complete(proto, base, "k", {
        model,
        messages: [{ role: "user", content: [{ type: "text", text: "Hello" }] }],
      });
      const want = exp.nonStreaming[proto];
      assert.equal(
        resp.content.filter((b) => b.type === "text").map((b) => b.text).join(""),
        want.text
      );
      assert.equal(
        resp.content.filter((b) => b.type === "thinking").map((b) => b.thinking).join(""),
        want.thinking
      );
      assert.equal(resp.finishReason, want.finishReason);
      assert.equal(resp.usage.inputTokens, want.inputTokens);
      if (want.cacheReadTokens !== undefined) {
        assert.equal(resp.usage.cacheReadTokens, want.cacheReadTokens);
      }

      currentCT = "text/event-stream";
      currentBody = readNativeFixture(sseFile);
      const events = await client.stream(proto, base, "k", {
        model,
        messages: [{ role: "user", content: [{ type: "text", text: "Hello" }] }],
      });
      assert.equal(
        events.filter((e) => e.type === "text_delta").map((e) => e.delta).join(""),
        exp.streamingText[proto]
      );
    }

    await assert.rejects(
      () =>
        client.complete("anthropic_messages", base, "k", {
          model: "claude-sonnet-4-5",
          messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
          responseFormat: { type: "json_schema", name: "x", schema: {} },
        }),
      (err: unknown) =>
        err instanceof LmpUnsupportedFeatureError && err.feature === "json_schema"
    );
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
