#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
echo "=== LingXiModelSDK 2.0 Real Host Benchmarks ==="
echo "Host: $(uname -srm)"
echo "Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""

echo "--- 1. Go Unified Translate+Normalize Benchmark ---"
(cd "$ROOT/sdk/go" && go test -bench=BenchmarkTranslateAndNormalizeGo -benchmem -benchtime=1s ./unified)

echo ""
echo "--- 2. Go Native Initialization and SSE Parsing ---"
(cd "$ROOT/sdk/go" && go test -run '^$' -bench 'Benchmark(ClientInitialization|SSEParsing)$' -benchmem -benchtime=1s ./native)

echo ""
echo "--- 3. Go Gateway Parallel Completion against loopback ---"
(cd "$ROOT/gateway" && go test -run '^$' -bench '^BenchmarkGatewayComplete$' -benchmem -benchtime=1s .)

echo ""
echo "--- 4. TypeScript LMP Translate+Normalize (10,000 iters) ---"
(cd "$ROOT" && node -) <<'JS'
import { performance } from "node:perf_hooks";
import { translateRequest, normalizeResponse } from "./sdk/typescript/src/unified/adapters.ts";
const req = { model: "gpt-4o", messages: [{ role: "user", content: [{ type: "text", text: "hello" }] }], thinking: { effort: "medium" } };
const raw = { id: "c1", choices: [{ message: { content: "ok" }, finish_reason: "stop" }], usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 } };
const t0 = performance.now();
for (let i = 0; i < 10000; i++) {
  translateRequest("openai_chat_completions", req);
  normalizeResponse("openai_chat_completions", raw);
}
const dt = performance.now() - t0;
console.log(`TS 10,000 translate+normalize: ${dt.toFixed(2)} ms (${((dt * 1000) / 10000).toFixed(2)} us/op)`);
JS

echo ""
echo "--- 5. Python + Rust C ABI (ctypes) Translate+Normalize (5,000 iters) ---"
PYTHONPATH="$ROOT/sdk/python/src" python3 - <<'PY'
import time
from lingxi_model_sdk.ffi_bridge import RustCoreBridge
bridge = RustCoreBridge()
assert bridge.available, "Rust core shared lib required"
req = {"model": "gpt-4o", "messages": [{"role": "user", "content": [{"type": "text", "text": "hello"}]}]}
raw = {"id": "c1", "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}
t0 = time.perf_counter()
for _ in range(5000):
    bridge.translate_request("openai_chat_completions", req)
    bridge.normalize_response("openai_chat_completions", raw)
dt = (time.perf_counter() - t0) * 1000.0
print(f"Python->Rust FFI 5,000 translate+normalize: {dt:.2f} ms ({dt*1000/5000:.2f} us/op)")
PY

echo ""
echo "--- 5b. Python + Rust C ABI Incremental Frame Normalization (30,000 frames) ---"
PYTHONPATH="$ROOT/sdk/python/src" python3 - <<'PY'
import ctypes, json, time
from lingxi_model_sdk.ffi_bridge import RustCoreBridge
bridge = RustCoreBridge()
assert bridge.available, "Rust core shared lib required"
frames = [
    {"event": "message_start", "data": json.dumps({"type": "message_start", "message": {"id": "m1", "usage": {"input_tokens": 5}}})},
    {"event": "content_block_delta", "data": json.dumps({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hello"}})},
]
lib = bridge._lib
assert lib is not None
push = lib.lingxi_lmp_normalizer_push
frame_bytes = [json.dumps(f).encode() for f in frames]
t0 = time.perf_counter()
for _ in range(10000):
    handle = lib.lingxi_lmp_normalizer_open(b"anthropic_messages")
    for payload in frame_bytes:
        bridge._take(push(handle, payload))
    bridge._take(lib.lingxi_lmp_normalizer_finish(handle))
    lib.lingxi_lmp_normalizer_close(handle)
dt = (time.perf_counter() - t0) * 1000.0
frames_per_round = len(frame_bytes) + 1
total = 10000 * frames_per_round
print(f"Python->Rust incremental normalizer: {dt:.2f} ms for {total} frames ({dt*1000/total:.2f} us/frame)")
PY

echo ""
echo "--- 6. Website CGO=0 Binary Size ---"
BINARY="$(mktemp "${TMPDIR:-/tmp}/lingxi-models-benchmark.XXXXXXXX")"
trap 'rm -f "$BINARY"' EXIT
(cd "$ROOT/website" && CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o "$BINARY" .)
printf 'Website binary size: %s bytes\n' "$(wc -c < "$BINARY" | tr -d '[:space:]')"