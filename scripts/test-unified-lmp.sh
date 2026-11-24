#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== [1/3] Rust Core LMP + C ABI ==="
(cd "$ROOT/core" && cargo test)

echo "=== [2/3] Go Unified LMP ==="
(cd "$ROOT/sdk/go" && go vet ./... && go test -race -v ./unified)

echo "=== [3/3] TypeScript Unified LMP ==="
(cd "$ROOT/sdk/typescript" && node --test tests/unified.test.ts)

echo "All Phase 4 Unified LMP suites passed."
