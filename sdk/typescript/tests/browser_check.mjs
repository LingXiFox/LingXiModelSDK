#!/usr/bin/env node
// Browser runtime check for the TypeScript SDK distribution.
//
// The SDK documents browser use for the Catalog, Native and Unified entry
// points. Node-only paths (filesystem cache storage, AWS SigV4 via node:crypto)
// are imported lazily and must not break a browser import graph.
//
// Serves the built `dist/` tree plus shared conformance fixtures from loopback,
// runs `browser_harness.html` in headless Chrome, and reads the JSON verdict the
// page writes into the DOM. Without a Chrome/Chromium binary it prints SKIP and
// exits 0 rather than pretending to pass.
//
// Usage: npm run build && node tests/browser_check.mjs

import { spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync, statSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const dist = path.resolve(here, "../dist");

const candidates = [
  process.env.CHROME_BIN,
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/opt/google/chrome/chrome",
  "/opt/google/chrome/google-chrome",
].filter((value) => typeof value === "string" && value.length > 0);
const browser = candidates.find((value) => existsSync(value));
if (!browser) {
  console.log("browser check SKIPPED: no Chrome/Chromium binary (set CHROME_BIN to enable)");
  process.exit(0);
}
if (!existsSync(path.join(dist, "unified", "index.js"))) {
  console.error("browser check FAILED: dist/ is missing; run `npm run build` first");
  process.exit(1);
}

const contentTypes = {
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".sse": "text/event-stream; charset=utf-8",
  ".html": "text/html; charset=utf-8",
};

// Loopback answers for the native clients: a JSON completion, an SSE stream and
// an Anthropic response. They replay the repository fixtures verbatim.
const mocks = {
  "/mock/v1/chat/completions": { type: "json", fixture: "openai_chat_response.json" },
  "/mock/v1/responses": { type: "json", fixture: "openai_responses_response.json" },
  "/mock/v1/messages": { type: "json", fixture: "anthropic_messages_response.json" },
  "/mock/v1beta/models/gemini-pro:streamGenerateContent": {
    type: "sse",
    fixture: "google_gemini_stream.sse",
  },
};

function serveFile(response, filePath) {
  const type = contentTypes[path.extname(filePath)] ?? "application/octet-stream";
  response.writeHead(200, { "Content-Type": type });
  response.end(readFileSync(filePath));
}

const server = http.createServer((request, response) => {
  const url = new URL(request.url ?? "/", "http://127.0.0.1");
  if (url.pathname === "/harness.html") {
    serveFile(response, path.join(here, "browser_harness.html"));
    return;
  }
  if (url.pathname.startsWith("/dist/")) {
    const file = path.join(dist, url.pathname.slice("/dist/".length));
    if (existsSync(file) && statSync(file).isFile()) {
      serveFile(response, file);
      return;
    }
  }
  if (url.pathname.startsWith("/fixtures/")) {
    const file = path.join(root, "conformance", url.pathname.slice("/fixtures/".length));
    if (existsSync(file) && statSync(file).isFile()) {
      serveFile(response, file);
      return;
    }
  }
  const mock = mocks[url.pathname];
  if (mock) {
    const fixture = readFileSync(path.join(root, "conformance", "native", mock.fixture));
    if (mock.type === "sse") {
      response.writeHead(200, { "Content-Type": "text/event-stream" });
      response.end(fixture);
      return;
    }
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(fixture);
    return;
  }
  response.writeHead(404, { "Content-Type": "application/json" });
  response.end('{"error":"not_found"}');
});

await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
const profile = path.join(process.env.TMPDIR ?? "/tmp", `lingxi-chrome-${process.pid}`);

const html = await new Promise((resolve, reject) => {
  const child = spawn(browser, [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-lcd-text",
    `--user-data-dir=${profile}`,
    "--virtual-time-budget=10000",
    "--dump-dom",
    `${origin}/harness.html`,
  ]);
  let out = "";
  child.stdout.on("data", (chunk) => {
    out += chunk.toString();
  });
  child.on("error", reject);
  child.on("close", () => resolve(out));
});

server.close();
rmSync(profile, { recursive: true, force: true });

const match = html.match(/<pre id="result">([\s\S]*?)<\/pre>/);
if (!match) {
  console.error("browser check FAILED: the harness page produced no result");
  process.exit(1);
}
const text = match[1]
  .replace(/&quot;/g, '"')
  .replace(/&lt;/g, "<")
  .replace(/&gt;/g, ">")
  .replace(/&amp;/g, "&");
let checks;
try {
  checks = JSON.parse(text);
} catch (error) {
  console.error("browser check FAILED: unreadable result");
  console.error(text.slice(0, 800));
  process.exit(1);
}
for (const check of checks) {
  console.log(`${check.ok ? "PASS" : "FAIL"} ${check.name}${check.ok ? "" : ": " + check.detail}`);
}
const failed = checks.filter((check) => !check.ok);
if (failed.length > 0) {
  console.error(`browser check FAILED: ${failed.length} of ${checks.length} checks`);
  process.exit(1);
}
console.log(`browser check passed: ${checks.length} checks running ${path.basename(browser)}`);