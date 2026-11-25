# LingXiModelSDK

Cross-language, cross-provider AI infrastructure and official SDK for the LingXi public model catalog (**https://models.lingxifox.cn/models.json**).

MIT License · Monorepo · In-process by default.

---

## Architecture (2.0)

LingXiModelSDK provides three independent, parallel entry points:

1. **Model Catalog** *(10 languages: Swift, Rust, Go, TS/JS, Python, Java, C++, C#, Kotlin, Dart)*
   Read-only model and provider metadata: identity, context/output limits, capabilities, pricing, and freshness. Requires no API keys or inference runtime.
   - Specification & Schemas: [specs/catalog-v2.md](specs/catalog-v2.md), [schemas/](schemas/catalog-v2.schema.json)
   - Language SDKs: [Swift](Sources/LingXiModelSDK/), [Rust](sdk/rust/), [Go](sdk/go/), [TypeScript/JS](sdk/typescript/), [Python](sdk/python/), [Java](sdk/java/), [C++](sdk/cpp/), [C#](sdk/csharp/), [Kotlin](sdk/kotlin/), [Dart](sdk/dart/)
2. **Native Provider** *(nine protocols with core mock evidence)*
   Direct upstream communication using each provider's native protocol, request/response types, AWS SigV4 signing, and SSE, NDJSON and Amazon Event Stream without forcing a unified abstraction.
   - Declarative Registry (327 providers): [registry/registry.json](registry/registry.json), [specs/provider-registry-v1.md](specs/provider-registry-v1.md)
   - Protocols: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Google Gemini, AWS Bedrock, Google Vertex AI, Azure OpenAI, Ollama, Cohere ([docs/feature-matrix.md](docs/feature-matrix.md))
3. **Unified Provider** *(opt-in LMP and language-specific alpha coverage)*
   Normalized LingXi Model Protocol (LMP) across providers. See the [feature matrix](docs/feature-matrix.md) for tested paths and incomplete mappings.
   - Optional HTTP Gateway: [gateway/](gateway/README.md), [specs/gateway-v1.md](specs/gateway-v1.md)
   - Benchmarks & Security Audit: [docs/performance-security-audit.md](docs/performance-security-audit.md)
   - Support Evidence Matrix: [docs/support-matrix.md](docs/support-matrix.md)
   - Local verification: `./scripts/test-all.sh`, [Phase 7 audit](docs/phase-7-audit.md) and the [Phase 8 roadmap section](ROADMAP.md)

See [ROADMAP.md](ROADMAP.md) and [specs/architecture-v2.md](specs/architecture-v2.md) for full details.

---

## Public Catalog Website (`website/`)

The `models.lingxifox.cn` service is implemented as a single-binary Go 1.23+ server (`CGO_ENABLED=0`, standard library only, embedded templates/CSS/JS/icons).

```sh
cd website
go test -race ./...
CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o lingxi-models .
./lingxi-models -listen 127.0.0.1:8080 -data-dir ./data
```

See [website/README.md](website/README.md) for CLI flags, sync behavior, and compatibility notes.

---

## Swift Model Catalog SDK (Shipped · `0.1.x`)

Foundation-only Swift Package Manager library (`macOS 13+`, `iOS 16+`, `tvOS 16+`, `watchOS 9+`).

### Installation

```swift
// Package.swift
dependencies: [
    .package(
        url: "https://github.com/LingXiFox/LingXiModelSDK.git",
        from: "0.1.0"
    )
]
```

```swift
.target(
    name: "MyApp",
    dependencies: [
        .product(
            name: "LingXiModelSDK",
            package: "LingXiModelSDK"
        )
    ]
)
```

### Usage

```swift
import LingXiModelSDK

let catalog = try await LingXiModelCatalog.load()

if let model = catalog.model(provider: "deepseek", id: "deepseek-v4-flash") {
    print(model.name)                          // display name
    print(model.contextWindow ?? 0)            // tokens
    print(model.maxOutputTokens ?? 0)          // tokens
    print(model.capabilities.reasoning)        // reasoning model?
    print(model.capabilities.toolCalling)      // tool calling?
    print(model.pricing.input ?? 0.0)          // USD per 1M input tokens
    print(model.pricing.output ?? 0.0)         // USD per 1M output tokens
}

let eligible = catalog.models(matching: ModelFilter(
    vision: true,
    minimumContextWindow: 200_000
))

print(catalog.revision.schemaVersion)
print(catalog.revision.catalogRevision)
print(catalog.revision.generatedAt as Any)
```

### Offline Use & Tests

```swift
let catalog = try LingXiModelCatalog.decoded(from: Data(contentsOf: url))
```

```bash
swift build
swift test
```

## License

MIT — see [LICENSE](LICENSE).