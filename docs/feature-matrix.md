# LingXiModelSDK 2.0 Alpha Coverage

The workflow builds and tests ten language SDKs. Entries describe the tested surface. The three entry points can be used without running the Gateway. The Catalog targets below are independent of the inference runtime.

| Language | Model Catalog | Provider Registry | Native Provider | Unified Provider |
| --- | --- | --- | --- | --- |
| Swift | `LingXiModelSDK`, catalog fixture conformance and cache tests | `LingXiProviderRegistry` | `LingXiNativeProvider`, prepared wire targets and incremental raw frames through the Rust core | `LingXiUnifiedProvider`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Rust | `sdk/rust` catalog fixture conformance | `lingxi_core::registry` | `lingxi_core::NativeClient`, nine adapters and buffered events | `lingxi_core::lmp`, nine adapters, buffered events, selected embed and rerank |
| Go | `sdk/go` catalog fixture conformance | `native.Registry` | `native.Client`, nine adapters and incremental events | `unified.Client`, nine adapters, incremental events, selected embed and rerank |
| TypeScript / JavaScript | Root import, catalog fixture conformance | `./registry` | `./native`, nine adapters and event generators. SigV4 uses Node crypto. | `./unified`, nine adapters, buffered and incremental interfaces, selected embed and rerank |
| Python | `lingxi_model_sdk` catalog fixture conformance | `.registry` | `.native`, prepared wire targets and incremental raw frames through the Rust core | `.unified`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Java | `LingXiModelCatalog`, fixture and loader tests | `ProviderRegistry` | `NativeClient`, prepared wire targets and incremental raw frames through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| C# | `LingXiModelCatalog`, fixture conformance and loader cache/ETag tests | `ProviderRegistry` | `NativeClient`, prepared requests and incremental raw frames through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Kotlin | `LingXiModelCatalog`, fixture conformance and loader cache/ETag tests | `ProviderRegistry` | `NativeClient`, prepared requests and incremental events through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| C++20 | `lingxi/catalog.hpp`, full fixture conformance and `catalog_loader.hpp` cache/ETag tests | `lingxi/registry.hpp` over the C ABI | `lingxi/native.hpp`, prepared wire targets and `StreamSession` raw frames through the Rust core | `lingxi/unified.hpp`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Dart 3 | `LingXiModelCatalog`, fixture conformance and loader cache/ETag tests | `ProviderRegistry` | `NativeClient`, prepared requests and incremental raw frames through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |

## Protocol coverage

Status is based on repository mock tests for the listed runtime paths. No entry has credentialed upstream evidence in this candidate. Language and model capabilities vary.

| Protocol | Core test coverage | Remaining limits |
| --- | --- | --- |
| OpenAI Chat Completions | Bearer JSON and SSE, basic text and reasoning responses, usage, selected embedding tests | Full parallel tool and multimodal parity |
| OpenAI Responses | Bearer JSON and typed SSE, reasoning summary examples | Encrypted reasoning state and follow-up tool parity |
| Anthropic Messages | API key, JSON and SSE, thinking and cache usage, explicit structured-output errors | Streamed tool arguments and signature replay parity |
| Google Gemini | API key, JSON and SSE, signed text and parallel tool replay, inline image, cached-content forwarding, usage, batch embeddings in Rust/Go/TS | Streamed tool and signature parity, model-specific thinking constraints, browser runtime verification |
| AWS Bedrock | SigV4 headers and Converse JSON, binary Amazon Event Stream in Rust/Go/TS and Rust-backed Python/C++, checksums, framing errors, selected normalized tool deltas and usage | Upstream SigV4 validation and model-specific reasoning support |
| Google Vertex AI | Bearer JSON and SSE fixtures, shared Gemini adapters, configured resource path | Dedicated Vertex embeddings are explicitly unsupported; live project routing remains unverified |
| Azure OpenAI | Deployment-scoped mock chat and API-key routing | Live resource and API-version verification |
| Ollama | Local chat and NDJSON fixtures, explicit unsupported structured-output behavior | Upstream local installation and OpenAI compatibility route validation |
| Cohere | Bearer chat, SSE fixtures and v2 rerank example | Provider-specific tool and usage parity |

The registry-level results and evidence are generated in [support-matrix.md](support-matrix.md).
