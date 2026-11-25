# LingXiModelSDK 2.0 Alpha Coverage

The workflow builds and tests ten language SDKs. Entries describe the tested surface. The three entry points can be used without running the Gateway. The Catalog targets below are independent of the inference runtime.

| Language | Model Catalog | Provider Registry | Native Provider | Unified Provider |
| --- | --- | --- | --- | --- |
| Swift | `LingXiModelSDK`, catalog fixture conformance and cache tests | `LingXiProviderRegistry` | `LingXiNativeProvider`, prepared wire targets and incremental raw frames through the Rust core | `LingXiUnifiedProvider`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Rust | `sdk/rust` catalog fixture conformance | `lingxi_core::registry` | `lingxi_core::NativeClient`, nine adapters and incremental `EventStreamHandle` | `lingxi_core::lmp`, nine adapters, incremental `LmpStreamHandle`, embed and rerank |
| Go | `sdk/go` catalog fixture conformance | `native.Registry` | `native.Client`, nine adapters and incremental events | `unified.Client`, nine adapters, incremental events, selected embed and rerank |
| TypeScript / JavaScript | Root import, catalog fixture conformance | `./registry` | `./native`, nine adapters and event generators. SigV4 uses Node crypto. | `./unified`, nine adapters, buffered and incremental interfaces, selected embed and rerank. Catalog, Native and Unified also run in a headless browser. |
| Python | `lingxi_model_sdk` catalog fixture conformance | `.registry` | `.native`, prepared wire targets and incremental raw frames through the Rust core | `.unified`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Java | `LingXiModelCatalog`, fixture and loader tests | `ProviderRegistry` | `NativeClient`, prepared wire targets and incremental raw frames through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| C# | `LingXiModelCatalog`, fixture conformance and loader cache/ETag tests | `ProviderRegistry` | `NativeClient`, prepared requests and incremental raw frames through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Kotlin | `LingXiModelCatalog`, fixture conformance and loader cache/ETag tests | `ProviderRegistry` | `NativeClient`, prepared requests and incremental events through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| C++20 | `lingxi/catalog.hpp`, full fixture conformance and `catalog_loader.hpp` cache/ETag tests | `lingxi/registry.hpp` over the C ABI | `lingxi/native.hpp`, prepared wire targets and `StreamSession` raw frames through the Rust core | `lingxi/unified.hpp`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |
| Dart 3 | `LingXiModelCatalog`, fixture conformance and loader cache/ETag tests | `ProviderRegistry` | `NativeClient`, prepared requests and incremental raw frames through the Rust core | `UnifiedClient`, LMP completion, incremental streaming, embed and rerank through the Rust core, explicit unsupported-feature errors |

Every package builds and installs as a distributable artifact: npm tarball, Python wheel, NuGet package, JAR, CMake package, and `cargo package` for both Rust crates. See [publishing.md](publishing.md) for the commands and the artifacts that were installed and exercised.

## Protocol coverage

Status is based on repository mock tests for the listed runtime paths. No entry has credentialed upstream evidence in this candidate; see [live-verification.md](live-verification.md) for the skipped probes and the reason. Language and model capabilities vary.

| Protocol | Core test coverage | Remaining limits |
| --- | --- | --- |
| OpenAI Chat Completions | Bearer JSON and SSE, text and reasoning responses, usage and cache usage, parallel tool calls, streamed tool arguments, multimodal input, structured output, selected embedding tests | Live upstream verification; provider-specific reasoning payloads beyond `reasoning_effort` |
| OpenAI Responses | Bearer JSON and typed SSE, reasoning summary and encrypted reasoning replay, `function_call`/`function_call_output` round trip, streamed arguments | Live upstream verification; server-side `store`/`previous_response_id` conversation state |
| Anthropic Messages | API key, JSON and SSE, thinking signatures, cache usage, parallel `tool_use`, streamed `input_json_delta`, `signature_delta`, explicit structured-output errors | Live upstream verification; interleaved-thinking beta request fields |
| Google Gemini | API key, JSON and SSE, `thoughtSignature` on text, thinking and tool parts, parallel function calls, inline image and audio, cached-content usage, batch embeddings in Rust/Go/TS, browser runtime check for the TypeScript SDK | Live upstream verification; model-specific thinking-level constraints |
| AWS Bedrock | SigV4 headers and Converse JSON, binary Amazon Event Stream in Rust/Go/TS and Rust-backed Python/C++, CRC and framing errors, reasoning signatures, parallel `toolUse`, `toolConfig.toolChoice`, image blocks, cache usage | Live SigV4 validation against AWS; model-specific reasoning support varies by foundation model |
| Google Vertex AI | Bearer JSON and SSE fixtures, shared Gemini adapters including signatures and tool replay, configured resource path | Dedicated Vertex embeddings are explicitly unsupported; live project routing remains unverified |
| Azure OpenAI | Deployment-scoped mock chat and API-key routing, parallel tools, multimodal input | Live resource and API-version verification |
| Ollama | Local chat and NDJSON fixtures, `tool_calls` and `tool_name` replay, thinking output, explicit unsupported structured-output behavior | Upstream local installation unverified; OpenAI compatibility route not exercised |
| Cohere | Bearer chat, SSE fixtures, thinking blocks, `tool_calls` replay, tool stream events, v2 rerank | Live upstream verification; provider-specific usage fields beyond `tokens` |

## Agent conformance

`conformance/agent/` holds one shared corpus for the nine protocols: the same multi-turn transcripts, normalized responses, streamed frames, translations and declared unsupported cases. Rust, Go and TypeScript replay it in their own test suites, and every language binding reaches the same normalizers through the Rust core. The corpus covers multi-turn tool loops, parallel tool calls, thinking and opaque signature state, prompt cache usage, multimodal input, streamed argument assembly, mid-flight cancellation and explicit unsupported-feature errors.

The registry-level results and evidence are generated in [support-matrix.md](support-matrix.md).
