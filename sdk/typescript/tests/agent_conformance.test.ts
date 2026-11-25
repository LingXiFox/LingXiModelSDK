import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import type { NativeProtocol } from "../src/native/index.ts";
import { LmpUnsupportedFeatureError } from "../src/unified/index.ts";
import {
  StreamNormalizer,
  normalizeResponse,
  translateRequest,
} from "../src/unified/adapters.ts";
import type { LmpRequest, LmpStreamEvent, LmpUsage } from "../src/unified/types.ts";

const rootDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const agentDir = path.join(rootDir, "conformance/agent");

function loadJson(...segments: string[]): any {
  return JSON.parse(fs.readFileSync(path.join(agentDir, ...segments), "utf8"));
}

const expectations = loadJson("expectations.json");
const protocols: NativeProtocol[] = expectations.protocols;

function requestFixture(id: string): LmpRequest {
  return loadJson("requests", `${id}.json`).request;
}

function streamEvents(protocol: NativeProtocol) {
  const doc = loadJson("streams", `${protocol}.json`);
  return doc.events as Array<{ event: string | null; data: string; id: string | null }>;
}

interface Aggregated {
  id: string | undefined;
  text: string;
  thinking: string;
  calls: Map<number, { id?: string; name?: string; arguments: string }>;
  signatures: Array<[string, string]>;
  finishReason: string | undefined;
  usage: LmpUsage | undefined;
}

function aggregate(events: LmpStreamEvent[]): Aggregated {
  const out: Aggregated = {
    id: undefined,
    text: "",
    thinking: "",
    calls: new Map(),
    signatures: [],
    finishReason: undefined,
    usage: undefined,
  };
  for (const event of events) {
    switch (event.type) {
      case "message_start":
        if (out.id === undefined) out.id = event.id;
        break;
      case "thinking_delta":
        out.thinking += event.delta;
        if (event.signature !== undefined) out.signatures.push(["thinking", event.signature]);
        break;
      case "text_delta":
        out.text += event.delta;
        if (event.signature !== undefined) out.signatures.push(["text", event.signature]);
        break;
      case "tool_call_delta": {
        let call = out.calls.get(event.index);
        if (call === undefined) {
          call = { arguments: "" };
          out.calls.set(event.index, call);
        }
        if (event.id !== undefined) call.id = event.id;
        if (event.name !== undefined) call.name = event.name;
        if (event.argumentsDelta !== undefined) call.arguments += event.argumentsDelta;
        if (event.signature !== undefined) out.signatures.push(["tool_call", event.signature]);
        break;
      }
      case "usage":
        out.usage = event.usage;
        break;
      case "message_end":
        out.finishReason = event.finishReason;
        break;
      case "error":
        throw new Error(`unexpected terminal stream error ${event.code}: ${event.message}`);
    }
  }
  return out;
}

/**
 * The shared fixtures are generated from the Rust core, which serializes an
 * absent optional counter as an explicit null. TypeScript omits it, so the
 * comparison drops nulls instead of weakening the type to allow them.
 */
function withoutNulls<T extends Record<string, unknown>>(value: T): Partial<T> {
  const out: Record<string, unknown> = {};
  for (const [key, entry] of Object.entries(value)) {
    if (entry !== null) out[key] = entry;
  }
  return out as Partial<T>;
}

/**
 * The shared agent fixtures are the contract for every LMP implementation.
 * These tests replay them through the TypeScript adapter layer alone, so a
 * regression in one protocol cannot be hidden by a transport detail.
 */
test("agent fixtures normalize to the shared LMP responses", () => {
  for (const protocol of protocols) {
    const expected = expectations.responses[protocol];
    const response = normalizeResponse(protocol, loadJson("responses", `${protocol}.json`));
    assert.equal(response.finishReason, expected.finishReason, `${protocol} finish reason`);

    const texts = response.content.filter((b) => b.type === "text").map((b) => b.text).join("");
    assert.equal(texts, expected.text, `${protocol} text`);
    const thinking = response.content.filter((b) => b.type === "thinking").map((b) => b.thinking).join("");
    assert.equal(thinking, expected.thinking, `${protocol} thinking`);

    const thinkingSignature = response.content.find((b) => b.type === "thinking")?.signature ?? null;
    assert.equal(thinkingSignature, expected.thinkingSignature, `${protocol} thinking signature`);

    const calls = response.content
      .filter((b) => b.type === "tool_call")
      .map((b) => ({ id: (b as { id: string }).id, name: (b as { name: string }).name, arguments: (b as { arguments: unknown }).arguments }));
    assert.deepEqual(calls, expected.toolCalls, `${protocol} tool calls`);

    assert.deepEqual(
      response.content.map((b) => b.type),
      expected.contentOrder,
      `${protocol} content order`,
    );
    assert.deepEqual(response.usage, withoutNulls(expected.usage), `${protocol} usage`);
  }
});

test("agent fixtures aggregate to the shared streamed LMP shape", () => {
  for (const protocol of protocols) {
    const expected = expectations.streams[protocol];
    const normalizer = new StreamNormalizer(protocol);
    const events: LmpStreamEvent[] = [];
    for (const frame of streamEvents(protocol)) {
      events.push(...normalizer.push(frame));
    }
    events.push(...normalizer.finish());
    const got = aggregate(events);

    assert.equal(got.text, expected.text, `${protocol} stream text`);
    assert.equal(got.thinking, expected.thinking, `${protocol} stream thinking`);
    assert.equal(got.id ?? null, expected.id, `${protocol} stream id`);
    assert.equal(got.finishReason, expected.finishReason, `${protocol} stream finish reason`);

    const calls = [...got.calls.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([index, call]) => ({
        index,
        id: call.id ?? null,
        name: call.name ?? null,
        arguments: call.arguments === "" ? null : JSON.parse(call.arguments),
      }));
    assert.deepEqual(calls, expected.toolCalls, `${protocol} streamed tool calls`);
    assert.deepEqual(got.signatures, expected.signatures, `${protocol} streamed signatures`);
    assert.deepEqual(got.usage, withoutNulls(expected.usage), `${protocol} stream usage`);

    // Exactly one terminal frame, however the upstream ends.
    assert.equal(events.filter((e) => e.type === "message_end").length, 1, `${protocol} message_end`);
  }
});

test("agent translations match the shared snapshot", () => {
  const snapshot = loadJson("translations.json");
  let checked = 0;
  for (const [requestId, perProtocol] of Object.entries(snapshot.requests as Record<string, any>)) {
    const request = requestFixture(requestId);
    for (const [protocol, expected] of Object.entries(perProtocol as Record<string, any>)) {
      const label = `${requestId} / ${protocol}`;
      if (expected.payload !== undefined) {
        assert.deepEqual(translateRequest(protocol as NativeProtocol, request), expected.payload, label);
      } else {
        assert.throws(
          () => translateRequest(protocol as NativeProtocol, request),
          (error: unknown) => {
            assert.ok(error instanceof LmpUnsupportedFeatureError, `${label} must raise a typed error`);
            assert.equal(error.code, expected.error.code, `${label} code`);
            assert.equal(error.message, expected.error.message, `${label} message`);
            return true;
          },
          label,
        );
      }
      checked += 1;
    }
  }
  assert.equal(checked, 27, "every protocol must cover every request");
});

test("normalized assistant turns replay every tool call and result", () => {
  const tools = requestFixture("multi-turn-tools").tools;
  for (const protocol of protocols) {
    const normalized = normalizeResponse(protocol, loadJson("responses", `${protocol}.json`));
    const callIds = normalized.content
      .filter((b) => b.type === "tool_call")
      .map((b) => (b as { id: string }).id);
    assert.equal(callIds.length, 2, `${protocol} fixture must expose two calls`);

    const request: LmpRequest = {
      model: "mock-1",
      messages: [
        { role: "user", content: [{ type: "text", text: "Compare Tokyo and Osaka." }] },
        { role: "assistant", content: normalized.content },
        { role: "tool", content: [{ type: "tool_result", toolCallId: callIds[0], content: "18 C" }] },
        { role: "tool", content: [{ type: "tool_result", toolCallId: callIds[1], content: "21 C" }] },
      ],
      tools,
    };
    const payload = JSON.stringify(translateRequest(protocol, request));
    assert.ok(payload.includes("18 C"), `${protocol} dropped the first tool result`);
    assert.ok(payload.includes("21 C"), `${protocol} dropped the second tool result`);
    assert.ok(payload.includes("get_weather"), `${protocol} dropped the tool definition`);
    // Ollama matches results by function name, so its synthesized ids are not
    // replayed; every other protocol keeps both call ids.
    if (protocol !== "ollama") {
      for (const id of callIds) assert.ok(payload.includes(id), `${protocol} dropped call id ${id}`);
    }
  }
});

test("structured output and multimodal input fail with a typed error", () => {
  const snapshot = loadJson("translations.json");
  const request = requestFixture("multimodal-structured");
  for (const protocol of protocols) {
    const expected = snapshot.requests["multimodal-structured"][protocol];
    if (expected.payload !== undefined) {
      const payload = JSON.stringify(translateRequest(protocol, request));
      assert.ok(
        payload.includes("receipt") || payload.includes("json_schema") || payload.includes("jsonSchema"),
        `${protocol} dropped structured output`,
      );
    } else {
      assert.throws(
        () => translateRequest(protocol, request),
        (error: unknown) => {
          assert.ok(error instanceof LmpUnsupportedFeatureError, `${protocol} must be typed`);
          assert.equal(error.code, expected.error.code, `${protocol} code`);
          return true;
        },
      );
    }
  }
});

test("forced tool choice is translated or rejected explicitly", () => {
  const snapshot = loadJson("translations.json");
  const request = requestFixture("forced-tool");
  for (const protocol of protocols) {
    const expected = snapshot.requests["forced-tool"][protocol];
    if (expected.payload !== undefined) {
      assert.deepEqual(translateRequest(protocol, request), expected.payload, `${protocol} forced tool choice`);
    } else {
      assert.throws(
        () => translateRequest(protocol, request),
        (error: unknown) => {
          assert.ok(error instanceof LmpUnsupportedFeatureError, `${protocol} must be typed`);
          assert.equal(error.code, expected.error.code, `${protocol} code`);
          return true;
        },
      );
    }
  }
});
