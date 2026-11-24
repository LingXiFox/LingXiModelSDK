import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import test from "node:test";
import { UnifiedClient, LmpUnsupportedFeatureError } from "../src/unified/index.ts";
import { ProviderRegistry } from "../src/registry/index.ts";

const FIXTURES = path.resolve("..", "..", "conformance", "native");

function readFixture(name: string): string {
  return fs.readFileSync(path.join(FIXTURES, name), "utf8");
}

function withServer(
  handler: (req: http.IncomingMessage, res: http.ServerResponse) => void
): Promise<{ url: string; close: () => Promise<void> }> {
  return new Promise((resolve) => {
    const srv = http.createServer(handler);
    srv.listen(0, "127.0.0.1", () => {
      const addr = srv.address() as { port: number };
      resolve({
        url: `http://127.0.0.1:${addr.port}`,
        close: () => new Promise((r) => srv.close(() => r())),
      });
    });
  });
}

test("registry standalone export loads 327 providers", () => {
  const raw = fs.readFileSync(path.resolve("..", "..", "registry", "registry.json"), "utf8");
  const reg = ProviderRegistry.fromJSON(raw);
  assert.ok(reg.size >= 320);
  assert.ok(reg.get("amazon-bedrock"));
  assert.ok(reg.get("cohere"));
});

test("tier-2 protocols, embed, rerank, redirect error, and unsupported feature", async () => {
  const client = new UnifiedClient();
  const bedrockResp = readFixture("aws_bedrock_response.json");
  const bedrockStream = readFixture("aws_bedrock_stream.ndjson");
  const vertexResp = readFixture("google_vertex_response.json");
  const azureResp = readFixture("azure_openai_response.json");
  const ollamaResp = readFixture("ollama_chat_response.json");
  const cohereResp = readFixture("cohere_chat_response.json");
  const cohereRerank = readFixture("cohere_rerank_response.json");
  const openaiEmbed = readFixture("openai_embed_response.json");

  const srv = await withServer((req, res) => {
    const url = req.url ?? "";
    if (url.includes("converse-stream")) {
      assert.ok((req.headers.authorization ?? "").startsWith("AWS4-HMAC-SHA256"));
      res.writeHead(200, { "Content-Type": "application/x-ndjson" });
      res.end(bedrockStream);
    } else if (url.includes("converse")) {
      assert.ok((req.headers.authorization ?? "").startsWith("AWS4-HMAC-SHA256"));
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(bedrockResp);
    } else if (url.includes("generateContent")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(vertexResp);
    } else if (url.includes("/deployments/")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(azureResp);
    } else if (url.includes("/api/chat")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(ollamaResp);
    } else if (url.includes("/v2/rerank")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(cohereRerank);
    } else if (url.includes("/v2/chat")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(cohereResp);
    } else if (url.includes("/embeddings")) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.end(openaiEmbed);
    } else if (url.includes("/redir")) {
      res.writeHead(302, { Location: "http://127.0.0.1:9/leak" });
      res.end();
    } else {
      res.writeHead(404);
      res.end();
    }
  });

  try {
    const bOut = await client.complete("aws_bedrock", srv.url, "AKIA:SECRET", {
      model: "anthropic.claude-3-5-sonnet",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
      thinking: { budgetTokens: 2048 },
    });
    assert.equal(bOut.content[0].type, "thinking");
    if (bOut.content[0].type === "thinking") {
      assert.equal(bOut.content[0].signature, "sig-bedrock-1");
    }

    const bEvs = await client.stream("aws_bedrock", srv.url, "AKIA:SECRET", {
      model: "anthropic.claude-3-5-sonnet",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
    });
    assert.ok(bEvs.some((e) => e.type === "text_delta" && e.delta === "Hello "));

    const vOut = await client.complete("google_vertex_ai", srv.url, "tok", {
      model: "gemini-2.5-pro",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
    });
    assert.equal(vOut.content[0].type, "thinking");
    if (vOut.content[0].type === "thinking") {
      assert.equal(vOut.content[0].signature, "sig-vertex-1");
    }

    const azOut = await client.complete("azure_openai", srv.url, "az-key", {
      model: "gpt-4o",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
    });
    assert.equal(azOut.content[0].type, "text");

    const olOut = await client.complete("ollama", srv.url, "", {
      model: "llama3.3",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
    });
    assert.equal(olOut.usage.totalTokens, 22);

    await assert.rejects(
      () =>
        client.complete("ollama", srv.url, "", {
          model: "llama3.3",
          messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
          responseFormat: { type: "json_schema", name: "x", schema: {} },
        }),
      LmpUnsupportedFeatureError
    );

    const coOut = await client.complete("cohere", srv.url, "co-key", {
      model: "command-a",
      messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
    });
    assert.ok(coOut.content.some((b) => b.type === "text"));

    const rr = await client.rerank("cohere", srv.url, "co-key", {
      model: "rerank-v3.5",
      query: "ts",
      documents: ["a", "b"],
      topN: 2,
    });
    assert.equal(rr.results[0].index, 1);

    const emb = await client.embed("openai_chat_completions", srv.url, "sk-test", {
      model: "text-embedding-3-small",
      input: ["hello", "world"],
    });
    assert.equal(emb.embeddings.length, 2);

    await assert.rejects(() =>
      client.complete("openai_chat_completions", `${srv.url}/redir`, "sk-secret", {
        model: "gpt-4o",
        messages: [{ role: "user", content: [{ type: "text", text: "hi" }] }],
      })
    );
  } finally {
    await srv.close();
  }
});
