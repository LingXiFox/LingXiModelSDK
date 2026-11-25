import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import test from "node:test";
import { UnifiedClient, LmpUnsupportedFeatureError } from "../src/unified/index.ts";
import { parseAmazonEventStream } from "../src/native/eventstream.ts";
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
  const bedrockStream = fs.readFileSync(path.join(FIXTURES, "aws_bedrock_stream.eventstream"));
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
      res.writeHead(200, { "Content-Type": "application/vnd.amazon.eventstream" });
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

test("Bedrock binary framing survives fragmentation and surfaces stream errors", async () => {
  const fixture = fs.readFileSync(path.join(FIXTURES, "aws_bedrock_stream.eventstream"));
  async function read(data: Uint8Array) {
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (let i = 0; i < data.length; i += 3) controller.enqueue(data.subarray(i, i + 3));
        controller.close();
      },
    });
    const events = [];
    for await (const event of parseAmazonEventStream(stream)) events.push(event);
    return events;
  }
  const events = await read(fixture);
  assert.equal(events.length, 8);
  assert.match(events[4].data, /東京/);
  await assert.rejects(read(fixture.subarray(0, -1)), /truncated/);
  const corrupt = Buffer.from(fixture);
  corrupt[corrupt.length - 1] ^= 1;
  await assert.rejects(read(corrupt), /checksum/);
  await assert.rejects(read(fs.readFileSync(path.join(FIXTURES, "aws_bedrock_error.eventstream"))), /error frame/);
});

test("Gemini replays signed parts and same-name parallel tool calls", async () => {
  const fixture = JSON.parse(fs.readFileSync(path.join(FIXTURES, "..", "agent", "gemini-tool-loop.json"), "utf8"));
  const seen: Record<string, unknown>[] = [];
  const upstream = await withServer(async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const payload = JSON.parse(Buffer.concat(chunks).toString());
    seen.push(payload);
    res.end(JSON.stringify(seen.length === 1 ? fixture.firstResponse : fixture.finalResponse));
  });
  try {
    const client = new UnifiedClient();
    const first = await client.complete("google_gemini", upstream.url, "fixture", fixture.request);
    assert.equal(first.usage.cacheReadTokens, 0);
    assert.equal(first.usage.reasoningTokens, 5);
    const tools = first.content.filter((b) => b.type === "tool_call");
    assert.equal(tools.length, 2);
    const final = await client.complete("google_gemini", upstream.url, "fixture", {
      ...fixture.request,
      messages: [
        ...fixture.request.messages,
        { role: "assistant", content: first.content },
        ...tools.map((tool) => ({
          role: "tool",
          content: [{ type: "tool_result", toolCallId: tool.id, content: tool.arguments.city === "東京" ? "18 C" : "21 C" }],
        })),
      ],
    });
    // An absent signature is omitted, not present as undefined, matching the
    // Rust core (`skip_serializing_if = "Option::is_none"`).
    assert.deepEqual(final.content, [{ type: "text", text: "Osaka is 3 C warmer." }]);
    assert.equal(final.usage.cacheReadTokens, 25);
    const [firstWire, lastWire] = seen as any[];
    assert.equal(firstWire.cachedContent, "cachedContents/mock-1");
    assert.deepEqual(firstWire.generationConfig.thinkingConfig, { thinkingLevel: "LOW", includeThoughts: true });
    assert.equal(firstWire.contents[0].parts[1].inlineData.mimeType, "image/png");
    assert.deepEqual(lastWire.contents[1], fixture.firstResponse.candidates[0].content);
    assert.deepEqual(lastWire.contents[2].parts, fixture.expectedToolResponses);
  } finally {
    await upstream.close();
  }
});

test("Gemini sends each batch embedding input and returns both vectors", async () => {
  const upstream = await withServer(async (req, res) => {
    assert.equal(req.url, "/v1beta/models/mock:batchEmbedContents");
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks).toString());
    assert.deepEqual(body.requests.map((request: any) => request.content.parts[0].text), ["first", "second"]);
    res.end('{"embeddings":[{"values":[0.1]},{"values":[0.2]}]}');
  });
  try {
    const response = await new UnifiedClient().embed("google_gemini", upstream.url, "fixture", {
      model: "models/mock", input: ["first", "second"],
    });
    assert.deepEqual(response.embeddings, [[0.1], [0.2]]);
  } finally {
    await upstream.close();
  }
});
