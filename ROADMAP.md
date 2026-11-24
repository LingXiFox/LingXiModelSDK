# LingXiModelSDK 2.0 Roadmap

LingXiModelSDK 2.0 evolves the repository from a single-language Swift model catalog package into a cross-language, cross-provider AI infrastructure monorepo under the MIT license.

## Architectural Invariants

1. **Three independent parallel entry points**:
   - **Model Catalog**: read-only model & provider metadata (`models.json` schema 2.0).
   - **Native Provider**: direct upstream communication using each provider's native protocol, payloads, and streaming events.
   - **Unified Provider**: opt-in normalized LingXi Model Protocol (LMP) with explicit unsupported-feature errors (no silent downgrade).
2. **In-process by default**: SDKs connect directly to upstream providers from the caller's process. HTTP Gateway is an optional, self-hosted subsystem.
3. **Evidence-based support claims**: upstream npm package names or catalog presence never imply protocol compatibility.

---

## Phase 1: Go Website Refactor (`website/`) — Completed (on `rewrite/2.0`)

- Replace static HTML + Tailwind CDN + Python `sync-models.py` with a single-binary Go 1.23+ server (`CGO_ENABLED=0`, stdlib only, `go:embed`).
- Preserve `/models.json`, `/publication.json`, schema `2.0`, ETag/`Last-Modified` conditional requests, CORS, ICP/MPS filing numbers, links, and original `beian-icon.png`.
- Add `/healthz`, `/sdk`, `/protocols`, `/providers`, modular templates, minimalist line-art UI, persistent atomic sync, shrink guard, `-check`, `-sync-once`, `SIGHUP` reload, and optional loopback admin sync.
- Provide systemd & Caddy snippet deployment templates (`website/deploy/`). Production switch requires explicit approval.

## Phase 2: Multi-Language Model Catalog SDK (`sdk/`, `schemas/`, `conformance/`) — Tier 1 Implemented

- Froze language-neutral catalog specification (`specs/catalog-v2.md`), JSON Schemas (`schemas/catalog-v2.schema.json`, `schemas/publication-v2.schema.json`), and shared conformance fixtures (`conformance/catalog/`).
- Implemented and conformance-tested all 6 Tier-1 native Catalog SDKs:
  - **Swift** (`Sources/LingXiModelSDK/`, root `Package.swift` backward-compatible)
  - **Go** (`sdk/go/`, zero external dependencies)
  - **Python** (`sdk/python/`, zero external dependencies)
  - **TypeScript / JavaScript** (`sdk/typescript/`, zero external dependencies)
  - **Rust** (`sdk/rust/`, minimal `serde`/`serde_json`/`reqwest` deps)
  - **Java** (`sdk/java/`, zero external dependencies, JDK 17+)
- **Tier 2** (pending): C#, Kotlin, C++, Dart.

## Phase 3: Provider Registry & Native Provider Clients (`registry/`, `core/`, `sdk/`) — Tier 1 Implemented

- Defined declarative registry & native wire specifications (`specs/provider-registry-v1.md`, `specs/native-provider-v1.md`, `schemas/provider-registry-v1.schema.json`).
- Built declarative registry (`registry/curated/*.json`, `registry/scripts/classify_catalog.py`, `registry/registry.json`) covering 326 catalog providers without ever inferring protocol compatibility from upstream `npm` package names.
- Implemented Native Provider clients (`core/` Rust, `sdk/go/native/`, `sdk/typescript/src/native/`) preserving native payloads, extensions, error bodies, and raw SSE events for all 4 Tier-1 protocols:
  - OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Google Gemini (all `Mock Tested` via `conformance/native/`).
- **Tier 2 protocols** (pending): AWS Bedrock, Google Vertex AI, Azure OpenAI, Ollama, Cohere.

## Phase 4: Unified Protocol Abstraction (`core/`, `specs/`, `conformance/`) — Implemented (Mock Tested)

- Froze **LingXi Model Protocol (LMP v1.0)** specification & schema (`specs/lmp-v1.md`, `schemas/lmp-v1.schema.json`) and evaluated multi-language bindings (`docs/bindings-evaluation.md`).
- Implemented bidirectional LMP adapters, explicit `UnsupportedFeature` guards, prompt cache/reasoning usage extraction, streaming normalization, and C ABI (`core/src/lmp/`, `core/src/ffi.rs`, `core/include/lingxi_core.h`).
- Implemented parallel native Go (`sdk/go/unified/`) and browser/Node TypeScript (`@lingxifox/model-sdk/unified`) clients verified against `conformance/lmp/expectations.json`.

## Phase 5: Optional Unified HTTP Gateway (`gateway/`) — Implemented (Mock Tested)

- Defined Gateway contract & security model (`specs/gateway-v1.md`).
- Implemented standalone Go 1.23+ HTTP/SSE gateway (`gateway/`, zero third-party dependencies):
  - Endpoints: `/v1/healthz`, `/v1/lmp/complete`, `/v1/lmp/stream`, `/v1/diagnostics/connect`.
  - Security: loopback bind by default, fail-closed mandatory Bearer auth + `-allow-hosts` egress allowlist for remote binds, SSRF protection, 16 MiB bounded requests, per-client token-bucket rate limiting, and zero credential/body logging.

## Phase 6: Conformance, Support Evidence & Release Automation (`conformance/`, `docs/`, `scripts/`) — Implemented

- Added evidence-backed support matrix (`conformance/evidence/support-matrix.json`, `scripts/generate-support-report.py`, `docs/support-matrix.md`) and embedded `website/static/provider-status.json` into the website `/providers` page.
- Added standalone protocol mock server (`conformance/mock-server/server.py`) and opt-in live verification script (`scripts/live-verify.sh`).
- Added full monorepo verification runner (`scripts/test-all.sh`), release checklist (`docs/publishing.md`), and manual `workflow_dispatch` release binary workflow (`.github/workflows/release-artifacts.yml`).

## Phase 7: Full Audit, 10-Language SDKs, 9 Protocols & `2.0.0-alpha.1` Readiness — Completed

- **10 Languages × 4 Modules**: Swift, Rust, Go, TypeScript/JS, Python (`ctypes` -> `liblingxi_core`), Java, C++20 (Rust C ABI), C# (.NET 8), Kotlin (JVM), Dart 3 — each providing independent Catalog, Provider Registry, Native Provider, and Unified Provider (LMP) modules.
- **9 Protocols (`Mock Tested`)**: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Google Gemini, AWS Bedrock (SigV4 + Converse/NDJSON), Google Vertex AI (`thoughtSignature`), Azure OpenAI, Ollama, Cohere (incl. `/v2/rerank`).
- **Gateway & Security Hardening**: Dial-time private/metadata IP SSRF protection, HTTP 3xx redirect blocking across all 10 SDKs, real-time `StreamEach` SSE flushing, `/v1/lmp/embed`, `/v1/lmp/rerank`, and secret redaction.
- **Website & Evidence**: Upgraded `website/` with `/gateway`, 10-language `/sdk`, 9-protocol `/protocols`, `2.0.0-alpha.1` release notes (`docs/release-2.0.0-alpha.1.md`), feature matrix (`docs/feature-matrix.md`), and host benchmarks + security audit (`docs/performance-security-audit.md`). Formal tagging/publishing and production cutover await explicit approval.

### Remaining Optional / Gated Items
- Tier-2 Catalog languages (C#, Kotlin, C++, Dart) and Tier-2 protocols (Bedrock, Vertex AI, Azure, Ollama, Cohere).
- Approved production server cutover (`models.lingxifox.cn`) and external package registry publishing.
