#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== 1. Website (Go 1.23+, CGO=0) ==="
(cd "$ROOT/website" && go vet ./... && go test -race ./...)

echo "=== 2. Rust Core (9 protocols + SigV4 + Agent Scenarios + C ABI) ==="
(cd "$ROOT/core" && cargo build && cargo test)

echo "=== 3. Go SDK (Catalog + Registry + Native + Unified) & Gateway ==="
(cd "$ROOT/sdk/go" && go vet ./... && go test -race ./...)
(cd "$ROOT/gateway" && go vet ./... && go test -race ./...)

echo "=== 4. TypeScript SDK ==="
(cd "$ROOT/sdk/typescript" && node --test tests/*.test.ts)

echo "=== 5. Python SDK (Catalog + Registry + Native + Unified + ctypes Rust FFI) ==="
PYTHONPATH="$ROOT/sdk/python/src" python3 -m unittest discover -s "$ROOT/sdk/python/tests"

echo "=== 6. Swift SDK (Catalog + Registry + Native + Unified) ==="
(cd "$ROOT" && swift test)

echo "=== 7. Java SDK (4 Modules) ==="
(cd "$ROOT/sdk/java" && mkdir -p build/classes && javac -d build/classes src/main/java/cn/lingxifox/modelsdk/*.java && java -cp build/classes cn.lingxifox.modelsdk.CatalogTestRunner && java -cp build/classes cn.lingxifox.modelsdk.ProviderTestRunner)

echo "=== 8. C++20 SDK (Catalog + Rust C ABI) ==="
(cd "$ROOT/sdk/cpp" && clang++ -std=c++20 -Iinclude -I../../core/include tests/test_cpp_sdk.cpp -L../../core/target/debug -Wl,-rpath,"$(pwd)/../../core/target/debug" -llingxi_core -o /tmp/test_cpp_sdk && /tmp/test_cpp_sdk && rm -f /tmp/test_cpp_sdk)

echo "=== 9. Support Matrix & Standalone Mock Server Smoke ==="
(cd "$ROOT" && python3 scripts/generate-support-report.py --check)
python3 - <<'PY' "$ROOT/conformance/mock-server/server.py"
import subprocess, sys, time, urllib.request, json

proc = subprocess.Popen([sys.executable, sys.argv[1], "--port", "18098"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
try:
    for _ in range(30):
        try:
            with urllib.request.urlopen("http://127.0.0.1:18098/healthz", timeout=1) as r:
                if r.status == 200:
                    break
        except Exception:
            time.sleep(0.1)
    else:
        raise RuntimeError("mock server failed to start")

    req = urllib.request.Request(
        "http://127.0.0.1:18098/v1/chat/completions",
        data=b'{"model":"gpt-5-mini","messages":[]}',
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=2) as r:
        doc = json.loads(r.read().decode("utf-8"))
        assert doc["id"] == "chatcmpl-mock-1"
    print("Standalone protocol mock server smoke check passed.")
finally:
    proc.terminate()
    proc.wait(timeout=5)
PY

echo "ALL LOCAL SUITES VERIFIED SUCCESSFULLY (C#, Kotlin, Dart verified via .github/workflows/all-sdks.yml)."
