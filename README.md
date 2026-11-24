# LingXiModelSDK

Cross-language, cross-provider AI infrastructure and official SDK for the LingXi public model catalog (**https://models.lingxifox.cn/models.json**).

MIT License · Monorepo · In-process by default.

---

## Architecture (2.0)

LingXiModelSDK provides three independent, parallel entry points:

1. **Model Catalog** *(Conformance-tested: Swift, Rust, TS/JS, Python, Go, Java · Planned Tier 2: C#, Kotlin, C++, Dart)*
   Read-only model and provider metadata: identity, context/output limits, capabilities, pricing, and freshness. Requires no API keys or inference runtime.
   - Specification & Schemas: [specs/catalog-v2.md](specs/catalog-v2.md), [schemas/](schemas/catalog-v2.schema.json)
   - Language SDKs: [Swift](Sources/LingXiModelSDK/), [Go](sdk/go/), [Python](sdk/python/), [TypeScript/JS](sdk/typescript/), [Rust](sdk/rust/), [Java](sdk/java/)
   - Run all 6 conformance suites: `./scripts/test-catalog-sdks.sh`
2. **Native Provider** *(Planned — Phase 3)*
   Direct upstream communication using each provider's native protocol, request/response types, and streaming events without forcing a unified abstraction.
3. **Unified Provider** *(Planned — Phase 4)*
   Opt-in normalized LingXi Model Protocol (LMP) across providers, with explicit unsupported-feature errors and no silent downgrades. An optional self-hosted HTTP Gateway is planned in Phase 5.

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
