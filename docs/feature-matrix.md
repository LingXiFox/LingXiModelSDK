# LingXiModelSDK 2.0 Alpha Coverage

The workflow builds and tests ten language SDKs. Entries describe the tested surface. The three entry points can be used without running the Gateway. The Catalog targets below are independent of the inference runtime.

| Language | Model Catalog | Provider Registry | Native Provider | Unified Provider |
| --- | --- | --- | --- | --- |
| Swift | `LingXiModelSDK`, catalog fixture conformance and cache tests | `LingXiProviderRegistry` | `LingXiNativeProvider`, JSON and buffered stream interfaces | `LingXiUnifiedProvider`, selected typed completion and rerank mappings |
| Rust | `sdk/rust` catalog fixture conformance | `lingxi_core::registry` | `lingxi_core::NativeClient`, nine adapters and buffered events | `lingxi_core::lmp`, nine adapters, buffered events, selected embed and rerank |
| Go | `sdk/go` catalog fixture conformance | `native.Registry` | `native.Client`, nine adapters and incremental events | `unified.Client`, nine adapters, incremental events, selected embed and rerank |
| TypeScript / JavaScript | Root import, catalog fixture conformance | `./registry` | `./native`, nine adapters and event generators. SigV4 uses Node crypto. | `./unified`, nine adapters, buffered and incremental interfaces, selected embed and rerank |
| Python | `lingxi_model_sdk` catalog fixture conformance | `.registry` | `.native`, stdlib HTTP, JSON and text event helpers | `.unified`, Rust adapter through ctypes, including buffered Bedrock streaming |
| Java | `LingXiModelCatalog`, fixture and loader tests | `ProviderRegistry` | `NativeClient`, HTTP and auth helpers | `UnifiedClient.completeText` and `rerank` return provider JSON. Full LMP parity remains open. |
| C# | `Catalog`, decode smoke test | `Registry` | `NativeClient`, HTTP and auth helpers | `CompleteTextAsync` and `RerankAsync` return provider JSON. Full LMP parity remains open. |
| Kotlin | `ModelCatalog`, decode smoke test | `ProviderRegistry` | `NativeClient`, HTTP and auth helpers | `completeText` and `rerank` return provider JSON. Full LMP parity remains open. |
| C++20 | `lingxi/catalog.hpp`, raw JSON and limited provider lookup | `lingxi/registry.hpp`, C ABI | `lingxi/native.hpp`, direct Rust runtime through the C ABI | Rust translation and blocking HTTP via the C ABI, with buffered events |
| Dart 3 | `lib/catalog.dart`, decode smoke test | `lib/registry.dart` | `lib/native.dart`, HTTP and auth helpers | `completeText` and `rerank` return provider JSON. Full LMP parity remains open. |

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
