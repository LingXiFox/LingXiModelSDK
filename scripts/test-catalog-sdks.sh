#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

echo "=== [1/6] Swift Catalog SDK ==="
(cd "$ROOT" && swift test)

echo "=== [2/6] Go Catalog SDK ==="
(cd "$ROOT/sdk/go" && go vet ./... && go test -race -v ./...)

echo "=== [3/6] Python Catalog SDK ==="
(cd "$ROOT/sdk/python" && PYTHONPATH=src python3 -m unittest discover -s tests -v)

echo "=== [4/6] TypeScript Catalog SDK ==="
(cd "$ROOT/sdk/typescript" && node --test tests/*.test.ts)

echo "=== [5/6] Rust Catalog SDK ==="
(cd "$ROOT/sdk/rust" && cargo test)

echo "=== [6/6] Java Catalog SDK ==="
(
  cd "$ROOT/sdk/java"
  mkdir -p build/classes
  javac -d build/classes src/main/java/cn/lingxifox/modelsdk/*.java src/test/java/cn/lingxifox/modelsdk/*.java
  java -cp build/classes cn.lingxifox.modelsdk.CatalogTestRunner
)

echo "All 6 Tier-1 Model Catalog SDK test suites passed."
