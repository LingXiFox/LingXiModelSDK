# Multi-Language Bindings Evaluation (Phase 4)

## 1. Architectural Split

- **Model Catalog (Phase 2)**: Zero FFI. Pure native implementations in Swift, Go, Python, TypeScript/JS, Rust, and Java ensure metadata-only consumers pay zero binary/FFI overhead.
- **Native & Unified Provider Core (`core/`)**: Centralized protocol translation, SSE parsing, capability guards, and C ABI (`core/src/ffi.rs`, `core/include/lingxi_core.h`).

## 2. Per-Ecosystem Evaluation

| Ecosystem | Recommended Binding Strategy | Rationale |
| --- | --- | --- |
| **Rust** | Direct `lingxi-core` crate (`rlib`) | Zero overhead, native `async`/Tokio, direct access to `NativeClient` and `UnifiedClient`. |
| **Swift / C++ / Python / Java / Kotlin / C#** | C ABI (`staticlib`/`cdylib` via `core/include/lingxi_core.h`) for LMP translation & capability validation, paired with native HTTP transport or UniFFI | Centralizes complex bidirectional LMP translation & error taxonomy in one Rust core while avoiding async runtime impedance mismatches across host event loops (`URLSession`, `asyncio`, `HttpClient`). |
| **TypeScript / JavaScript (Node & Browser)** | Native TypeScript (`@lingxifox/model-sdk/unified`, zero FFI; optional WASM build of `core` for pure translation) | Browsers cannot load `cdylib` C ABI; native `fetch` + `AbortSignal` + `ReadableStream` works identically in Chrome/Safari/Node without WASM cold-start overhead. |
| **Go** | Pure Go (`sdk/go/unified`) using `net/http` + `context.Context` | Avoids `CGO_ENABLED=1` requirement so Go callers retain single-binary cross-compilation (`CGO_ENABLED=0`), while sharing `conformance/lmp/` parity tests with `core/`. |

## 3. Verified C ABI Surface

`core/include/lingxi_core.h` exports 21 symbols in three groups. All of them were exercised from at least two host languages (Python ctypes and Java FFM) and from the C++ header-only binding.

| Group | Symbols | Scope |
| --- | --- | --- |
| Translation and normalization | `lingxi_lmp_translate_request`, `lingxi_lmp_normalize_response`, `lingxi_lmp_normalize_stream_events`, `lingxi_string_free` | Pure JSON in / out, no I/O, no runtime |
| Wire preparation and one-shot requests | `lingxi_native_prepare`, `lingxi_unified_prepare`, `lingxi_unified_embed`, `lingxi_unified_rerank`, `lingxi_registry_resolve_endpoint`, `lingxi_runtime_request` | Preparation is pure; `lingxi_runtime_request` blocks and must not be called from an async runtime |
| Streaming | `lingxi_stream_open`, `lingxi_stream_open_error`, `lingxi_stream_next`, `lingxi_stream_cancel`, `lingxi_stream_close`, `lingxi_stream_last_error`, `lingxi_lmp_normalizer_open`, `lingxi_normalizer_open_error`, `lingxi_lmp_normalizer_push`, `lingxi_lmp_normalizer_finish`, `lingxi_lmp_normalizer_close` | `lingxi_stream_*` owns a Tokio runtime on its own thread and pulls events on demand; `lingxi_lmp_normalizer_*` is state-only and safe on a host event loop |

Stream handles are monotonic ids checked against a registry, not raw pointers, so a NULL, released or forged handle is rejected instead of dereferenced; `lingxi_stream_close` is idempotent and safe concurrently with `lingxi_stream_next`. The two normalizer paths exist because a host that owns its own HTTP transport cannot hand the core a complete event array: the session API normalizes one frame at a time with the same state machine the core uses internally.