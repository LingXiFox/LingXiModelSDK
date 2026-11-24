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

## Phase 5: Optional Unified HTTP Gateway (`gateway/`)

- Wrap the unified protocol core in an optional self-hosted HTTP server.
- Support versioned HTTP endpoints, SSE streaming, authentication, connection diagnostics, rate limiting, and cancellation.
- Security defaults: loopback bind by default, mandatory auth + egress allowlists + tenant isolation for remote deployments, no credential/body logging, public catalog website never accepts provider API keys or inference traffic.

## Phase 6: Conformance, Release Automation & Ecosystem Publishing

- Track per-provider/protocol/capability evidence across 5 tiers: `Discovered`, `Compatible`, `Mock Tested`, `Live Verified`, `Full Support`.
- Build cross-language conformance runner, protocol mock servers, optional live API verification suites, and automated per-ecosystem package publishing.
