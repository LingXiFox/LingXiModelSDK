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

`core/include/lingxi_core.h` exports pure deterministic JSON-in / JSON-out functions (`lingxi_lmp_translate_request`, `lingxi_lmp_normalize_response`, `lingxi_string_free`) so host languages can invoke Rust's canonical LMP translation without embedding Tokio or reqwest inside C callbacks.
