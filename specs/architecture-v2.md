# LingXiModelSDK 2.0 architecture

Status: proposed / phased implementation. No runtime endpoint is advertised as implemented until its tests pass.

## Three independent public entry points

1. **Catalog**: read-only model/provider metadata. Existing https://models.lingxifox.cn/models.json and /publication.json remain compatible (schema 2.0). No inference, credentials, or runtime dependency required. A consumer can use raw HTTP or a language-native catalog SDK.
2. **Native provider client**: connect directly to a selected provider using its actual API protocol, endpoint and caller-supplied credentials. Preserve native payloads, responses, streaming, extensions and error details. No conversion to the unified model protocol required.
3. **Unified provider client**: opt-in normalized request, response and stream interfaces. Choose a provider/protocol adapter, map capabilities, and communicate directly with upstream by default. The caller may optionally deploy a separate gateway exposing the same unified contract over HTTP. Neither catalog nor native access depends on this gateway.

A gateway is an optional deployment product, not the public catalog website. The SDK must be usable as an in-process library without an account with LingXi or routing prompts through lingxifox.cn. Native and unified clients share the transport and authentication layer, but native access bypasses semantic mapping.

## Components

- website/: Go standalone binary, embedded templates/assets/filing information, upstream sync and persistent model snapshots.
- specs/ and schemas/: language-neutral catalog, provider registry, native and unified interface contracts.
- sdk/: Swift (existing root SPM package remains compatible), Rust, TypeScript/JavaScript, Python, Go, Java, then C#, Kotlin, C++, Dart. The catalog layer should use lightweight native code and only fetch published metadata.
- core/: proposed Rust transport/protocol core with C ABI or other language bindings for provider clients where practical. Browser/WASM and mobile constraints need explicit verification. Do not require FFI for metadata-only clients.
- registry/: curated protocol, authentication, capabilities, and compatibility overrides separate from upstream model metadata.
- conformance/: shared fixtures, expected normalized events, mock upstreams and compatibility tests.
- gateway/: optional self-hosted HTTP server for unified and explicitly scoped native calls; no public inference service enabled by default.

## Registry and protocol rules

A model or provider listed in models.json is not proof that its inference API is supported. Preserve source provenance and unknown fields. Registry entries may declare multiple protocol options, endpoint templates, authentication types and feature mappings. Provider-specific overrides must be declarative where possible, with reviewed adapter extensions when necessary. Do not infer runtime behavior from upstream package names alone. A known endpoint must be explicitly chosen or trusted before credentials are sent; remote registry updates cannot silently redirect credential destinations.

Compatibility is per provider/protocol/capability: discovered, declared compatible, mock tested, live verified. Record evidence and date. No silent feature downgrade; expose unsupported features, raw provider extensions and request-scoped opt-in fallback. Normalize tool-call IDs, content blocks, finish reason, usage and streaming lifecycle while preserving opaque provider state where required. The Agent owns tool execution, approval, conversation and context orchestration.

Gateway security: loopback binding by default; authentication required for non-local deployment, egress allowlists, tenant isolation, no credential/body logging, bounded requests and cancellation. No arbitrary URL proxying and no automatic retries after ambiguous or partially delivered inference. User-configured direct connections must be distinct from shared hosted endpoints.

## Milestones

P1 — Website: Go single binary, preserve public URL/schema and legal footer, offline seed/recovery, sync validation and ETag, deployment smoke test with the production snapshot. Do not replace live Caddy configuration until explicitly approved.

P2 — Multi-language catalog SDK: freeze catalog semantics and shared fixtures; implement and test six primary ecosystems, then remaining four. Existing Swift API stays compatible; unknown fields, null versus zero, conditional requests, stale-cache behavior, natural sorting and offline use are required. Publish only packages that pass their ecosystem tests. No inference dependency.

P3 — Provider registry and native access: define endpoint/auth metadata and protocol capability claims; implement native access for OpenAI Chat, Responses, Anthropic Messages and Gemini, then other protocols according to evidence. Direct provider mode must preserve payloads and native events.

P4 — Unified in-process protocol: shared message/response/tool/usage/event types, capability-aware adapters and conformance tests. Keep native provider and catalog paths fully functional and separately importable. Evaluate bindings per language.

P5 — Optional gateway: versioned HTTP interface, SSE, authentication, rate limits, diagnostics, optional self-hosted deployment and explicit native passthrough policy. A gateway outage must not prevent use of direct SDK modes.

P6 — Publication and integration: multi-language packages, documentation, compatibility evidence and Agent integration. No claim of 300+ live-tested providers without evidence.

## Acceptance

- A catalog-only application can query model metadata without a key or provider runtime.
- A native client can call a supported upstream without the unified layer or a LingXi gateway.
- A unified client can switch supported providers through the same interface, with explicit errors for unmappable capabilities.
- All three paths can coexist in one application; no forced migration for existing consumers.
- Mock and live verification statuses are distinct; provider credentials never go to the public catalog service.
