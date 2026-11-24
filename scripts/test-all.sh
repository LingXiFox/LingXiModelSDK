#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== 1. Website (Go 1.23+, CGO=0) ==="
(cd "$ROOT/website" && go vet ./... && go test -race ./...)

echo "=== 2. Rust Core (9 protocols + SigV4 + Agent Scenarios + C ABI) ==="
(cd "$ROOT/core" && cargo build --locked && cargo test --locked)

(cd "$ROOT/sdk/rust" && cargo test --locked)

echo "=== 3. Go SDK (Catalog + Registry + Native + Unified) & Gateway ==="
(cd "$ROOT/sdk/go" && go vet ./... && go test -race ./...)
(cd "$ROOT/gateway" && go vet ./... && go test -race ./...)

echo "=== 4. TypeScript SDK ==="
(cd "$ROOT/sdk/typescript" && node --test tests/*.test.ts && npm run build)

echo "=== 5. Python SDK (Catalog + Registry + Native + Unified + ctypes Rust FFI) ==="
LINGXI_REQUIRE_FFI=1 PYTHONPATH="$ROOT/sdk/python/src" python3 -m unittest discover -s "$ROOT/sdk/python/tests"

echo "=== 6. Swift SDK (Catalog + Registry + Native + Unified) ==="
(cd "$ROOT" && swift test)

echo "=== 7. Java SDK (4 Modules) ==="
(cd "$ROOT/sdk/java" && mkdir -p build/classes && javac -d build/classes src/main/java/cn/lingxifox/modelsdk/*.java src/test/java/cn/lingxifox/modelsdk/*.java && java -cp build/classes cn.lingxifox.modelsdk.CatalogTestRunner && java -cp build/classes cn.lingxifox.modelsdk.ProviderTestRunner)

echo "=== 8. C++20 SDK (Catalog + Rust C ABI) ==="
(cd "$ROOT/sdk/cpp" && clang++ -std=c++20 -Iinclude -I../../core/include tests/test_cpp_sdk.cpp -L../../core/target/debug -Wl,-rpath,"$(pwd)/../../core/target/debug" -llingxi_core -o /tmp/test_cpp_sdk && /tmp/test_cpp_sdk && rm -f /tmp/test_cpp_sdk)

echo "=== 9. Support Matrix & Standalone Mock Server Smoke ==="
(cd "$ROOT" && python3 scripts/generate-support-report.py --check && PYTHONPATH=registry/scripts python3 -m unittest discover -s registry/scripts)
python3 - <<'PY' "$ROOT/conformance/mock-server/server.py"
import json, select, subprocess, sys, urllib.request

proc = subprocess.Popen([sys.executable, sys.argv[1], "--port", "0"], stdout=subprocess.PIPE, text=True)
try:
    if not select.select([proc.stdout], [], [], 5)[0]:
        raise RuntimeError("mock server failed to start")
    base = proc.stdout.readline().rstrip().split()[-1]
    cases = {
        "/v1/chat/completions": "application/json",
        "/v1/responses": "application/json",
        "/v1/messages": "application/json",
        "/v1beta/models/chat:streamGenerateContent?alt=sse": "text/event-stream",
        "/v1/projects/test/locations/global/publishers/google/models/chat:streamGenerateContent?alt=sse": "text/event-stream",
        "/model/chat/converse-stream": "application/vnd.amazon.eventstream",
        "/openai/deployments/chat/chat/completions?api-version=2024-10-21": "application/json",
        "/api/chat": "application/json",
        "/v2/chat": "application/json",
        "/v2/rerank": "application/json",
    }
    for path, content_type in cases.items():
        req = urllib.request.Request(base + path, data=b"{}", headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=2) as resp:
            assert resp.headers.get_content_type() == content_type, path
            if content_type == "application/json":
                assert json.load(resp), path
            else:
                assert resp.read(), path
    print("Standalone mock server smoke checks passed for all nine protocols.")
finally:
    proc.terminate()
    proc.wait(timeout=5)
PY

echo "Local suites passed. C#, Kotlin, and Dart run in .github/workflows/all-sdks.yml."
