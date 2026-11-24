#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== [1/4] Provider Registry Invariants ==="
(cd "$ROOT" && PYTHONPATH=registry/scripts python3 -m unittest discover -s registry/scripts -v)

echo "=== [2/4] Rust Core (Registry + 4 Native Protocols) ==="
(cd "$ROOT/core" && cargo test)

echo "=== [3/4] Go Native Provider ==="
(cd "$ROOT/sdk/go" && go vet ./... && go test -race -v ./native)

echo "=== [4/4] TypeScript Native Provider ==="
(cd "$ROOT/sdk/typescript" && node --test tests/native.test.ts)

echo "All Phase 3 Provider Registry & Native Provider suites passed."
