# AGENTS.md — LingXiModelSDK 2.0

## Repository Overview

LingXiModelSDK is a MIT-licensed monorepo upgrading from a Swift-only model catalog package into a cross-language, cross-provider AI infrastructure.

### Three Independent Parallel Entry Points

1. **Model Catalog**: read-only model & provider metadata (`https://models.lingxifox.cn/models.json`, schema `2.0`). Zero inference dependencies or API keys.
2. **Native Provider**: direct communication with upstream providers using native wire protocols, payloads, and streaming events.
3. **Unified Provider**: opt-in normalized LingXi Model Protocol (LMP) with explicit errors for unmappable capabilities (no silent downgrade).

All three entry points must remain independently usable. By default, SDKs run in-process and connect directly to upstream providers; the HTTP Gateway is an optional subsystem.

## Directory Layout

- `Sources/`, `Tests/`, `Package.swift`: existing Swift Catalog SDK (must remain backward compatible).
- `website/`: Go 1.23+ standalone catalog website (`models.lingxifox.cn`) with `go:embed` assets and atomic upstream sync.
- `specs/`, `schemas/`: language-neutral specifications and JSON schemas.
- `sdk/`, `core/`, `registry/`, `gateway/`, `codegen/`, `conformance/`, `scripts/`, `docs/`: staged 2.0 directories (see `ROADMAP.md`).

## Verification Commands

- **Go website**:
  ```sh
  cd website && go vet ./... && go test -race ./... && CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o lingxi-models .
  ```
- **Swift SDK**:
  ```sh
  swift test
  ```

## Strict Engineering Rules

1. **No empty stubs or fake verification**: never ship placeholder implementations, mock data disguised as live support, or claim a provider/protocol is verified without evidence.
2. **Never infer protocol compatibility from npm package names**: provider discovery in `models.json` (`Discovered`) and runtime protocol verification (`Compatible`, `Mock Tested`, `Live Verified`, `Full Support`) are strictly separate.
3. **Preserve unknown fields & three-state semantics**: strictly distinguish `null`/omitted (unknown), `0` (e.g., free pricing), and `false` (unsupported).
4. **No unauthorized production changes**: never overwrite live `/etc/caddy/Caddyfile`, restart production services, or publish packages without explicit approval.
5. **Commit hygiene**: never use `--no-verify`; clean up temporary files in `/tmp` after verification.
