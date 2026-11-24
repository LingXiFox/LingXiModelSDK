#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== Phase 1: Website ==="
(cd "$ROOT/website" && go vet ./... && go test -race ./...)

echo "=== Phase 2: Multi-Language Catalog SDKs ==="
"$ROOT/scripts/test-catalog-sdks.sh"

echo "=== Phase 3: Provider Registry & Native Providers ==="
"$ROOT/scripts/test-native-providers.sh"

echo "=== Phase 4: Unified LMP & C ABI ==="
"$ROOT/scripts/test-unified-lmp.sh"

echo "=== Phase 5: Optional HTTP Gateway ==="
(cd "$ROOT/gateway" && go vet ./... && go test -race -v ./...)

echo "=== Phase 6: Support Matrix & Standalone Mock Server Smoke ==="
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

echo "ALL PHASES (1-6) VERIFIED SUCCESSFULLY."
