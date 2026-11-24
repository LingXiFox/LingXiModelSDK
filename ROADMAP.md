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

## Phase 3: Provider Registry & Native Provider Clients (`registry/`, `core/`, `sdk/`)

- Build declarative `registry/` describing Provider ID, official endpoints, supported protocols, auth schemes, custom headers/params, thinking levels/budgets, model overrides, and verification status.
- Implement **Native Provider** clients preserving native request/response/SSE payloads and extensions:
  - **Tier 1 protocols**: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Google Gemini.
  - **Tier 2 protocols**: AWS Bedrock, Google Vertex AI, Azure OpenAI, Ollama, Cohere.
- Establish automated classification & declarative onboarding for 300+ catalog providers without conflating discovery with verification.

## Phase 4: Unified Protocol Abstraction (`core/`, `specs/`, `conformance/`)

- Define **LingXi Model Protocol (LMP)** covering requests/responses, multi-turn & multimodal messages, streaming events, thinking content & levels, tool definitions/calls/results, structured outputs, token usage, prompt cache, typed errors, timeouts/cancellation, connection pooling, embeddings, and reranking.
- Evaluate Rust shared protocol core + language bindings (C ABI / UniFFI / WASM / native) per target ecosystem (server, mobile, browser).
- Preserve native escape hatches; return explicit unsupported errors where semantic mapping does not exist.
- Leave session state, tool execution, permission approval, context compaction, and workflow orchestration to the consuming Agent.

## Phase 5: Optional Unified HTTP Gateway (`gateway/`)

- Wrap the unified protocol core in an optional self-hosted HTTP server.
- Support versioned HTTP endpoints, SSE streaming, authentication, connection diagnostics, rate limiting, and cancellation.
- Security defaults: loopback bind by default, mandatory auth + egress allowlists + tenant isolation for remote deployments, no credential/body logging, public catalog website never accepts provider API keys or inference traffic.

## Phase 6: Conformance, Release Automation & Ecosystem Publishing

- Track per-provider/protocol/capability evidence across 5 tiers: `Discovered`, `Compatible`, `Mock Tested`, `Live Verified`, `Full Support`.
- Build cross-language conformance runner, protocol mock servers, optional live API verification suites, and automated per-ecosystem package publishing.
