# LingXiModelSDK 2.0 Feature Coverage Matrix

## 1. Ten-Language SDK Matrix

All 10 languages maintain three independent entry points (**Model Catalog** has zero inference runtime dependencies):

| Language | Catalog | Provider Registry | Native Provider | Unified Provider (LMP) | Test / CI Verification |
| --- | --- | --- | --- | --- | --- |
| **Swift** | `LingXiModelSDK` | `LingXiProviderRegistry` | `LingXiNativeProvider` | `LingXiUnifiedProvider` | `swift test` (macOS + Linux CI) |
| **Rust** | `sdk/rust` | `core::registry` | `core::native` (9 protocols + SigV4) | `core::lmp` + `core::ffi` (C ABI) | `cargo test` |
| **Go** | `sdk/go` | `sdk/go/native` | `sdk/go/native` (9 protocols + `StreamEach`) | `sdk/go/unified` (9 protocols + `Embed`/`Rerank`) | `go test -race ./...` |
| **TypeScript / JS** | `sdk/typescript` (`.`) | `./registry` | `./native` (9 protocols + SigV4) | `./unified` (9 protocols + `embed`/`rerank`) | `node --test tests/*.test.ts` |
| **Python** | `lingxi_model_sdk.catalog` | `.registry` | `.native` (9 protocols + SigV4) | `.unified` (`ctypes` -> `liblingxi_core`) | `unittest` (host + CI) |
| **Java** | `LingXiModelCatalog` | `ProviderRegistry` | `NativeClient` (9 protocols + SigV4) | `UnifiedClient` | `javac` + `java` (host + CI) |
| **C++20** | `lingxi/catalog.hpp` | `lingxi/registry.hpp` | `lingxi/native.hpp` | `lingxi/unified.hpp` (Rust C ABI) | `clang++ -std=c++20` (host + CI) |
| **C# (.NET 8)** | `LingXiModelSDK.Catalog` | `.Registry` | `.Native` (9 protocols + SigV4) | `.Unified` | `dotnet run` (CI) |
| **Kotlin (JVM)** | `ModelCatalog` | `ProviderRegistry` | `NativeClient` (9 protocols + SigV4) | `UnifiedClient` | `kotlinc` + `java` (CI) |
| **Dart 3** | `lib/catalog.dart` | `lib/registry.dart` | `lib/native.dart` | `lib/unified.dart` | `dart run test/sdk_test.dart` (CI) |

## 2. Nine-Protocol Capability Matrix

| Protocol | Auth | Chat & Stream | Tools & Parallel Calls | Thinking & Signature | Prompt Cache | Embed / Rerank | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `openai_chat_completions` | Bearer | SSE | Yes | `reasoning_effort` (`budget_tokens` -> explicit error) | `cached_tokens` | Embed: Yes / Rerank: Unsupported | `mock_tested` |
| `openai_responses` | Bearer | SSE | Yes | `reasoning.summary` (`budget_tokens` -> explicit error) | `cached_tokens` | Unsupported | `mock_tested` |
| `anthropic_messages` | `x-api-key` | SSE | Yes | `budget_tokens` + `signature` (`json_schema` -> explicit error) | `cache_read` / `cache_creation` | Unsupported | `mock_tested` |
| `google_gemini` | `x-goog-api-key` | SSE | Yes | `thought` + `thoughtSignature` | `cachedContentTokenCount` | Embed: Yes / Rerank: Unsupported | `mock_tested` |
| `aws_bedrock` | AWS SigV4 | NDJSON | `toolUse` / `toolResult` | `budget_tokens` + `signature` | `cacheRead` / `cacheWrite` | Unsupported | `mock_tested` |
| `google_vertex_ai` | Bearer | SSE | Yes | `thought` + `thoughtSignature` | `cachedContentTokenCount` | Embed: Yes / Rerank: Unsupported | `mock_tested` |
| `azure_openai` | `api-key` | SSE | Yes | `reasoning_effort` | `cached_tokens` | Embed: Yes / Rerank: Unsupported | `mock_tested` |
| `ollama` | None | NDJSON | Text/Vision | `think: true` (`json_schema` -> explicit error) | — | Embed: Yes / Rerank: Unsupported | `mock_tested` |
| `cohere` | Bearer | SSE | Text/Thinking | `token_budget` | — | Embed: Yes / Rerank: **Yes** (`/v2/rerank`) | `mock_tested` |
