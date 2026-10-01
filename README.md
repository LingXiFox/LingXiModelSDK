# LingXiModelSDK

Official Swift SDK for the LingXi public model catalog:
**https://models.lingxifox.cn/models.json**

MIT · Swift Package Manager · Foundation only · no LingXiAgent required.

---

## What this is, and what it is not

```text
LingXiModelSDK  = developer interface for the public model catalog
models.json     = data contract
LingXiAgent     = one consumer of this SDK, like any other
```

It answers questions about **models**: who publishes them, what their identity
is, how large their context and output windows are, what they cost, and what
they can do.

It deliberately does **not** do inference. There is no chat client, no streaming
loop, no API key handling, no provider auth, and no agent runtime here — those
belong to whatever consumes the catalog, and mixing them in would mean a
developer who only wants to read a context window has to install a runtime.

| In scope | Not in scope |
| --- | --- |
| fetch, ETag revalidation, cache, freshness | LLM inference |
| catalog schema decode and version compatibility | OpenAI / Anthropic clients |
| provider and model lookup, search, filtering | streaming or tool-calling runtime |
| limits, capabilities, pricing, revision metadata | API key management |
| one documented ordering rule | agent loops, sessions, permissions |

Nothing in this package depends on `LingXiAgent`, `LingXiCore`,
`LingXiApplication`, `LingXiClient`, `LingXiProtocol` or `LingXiPluginSDK`.

## Installation

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

In Xcode: **File → Add Package Dependencies…** and paste
`https://github.com/LingXiFox/LingXiModelSDK.git`.

## Usage

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
```

Filtering by published metadata:

```swift
let eligible = catalog.models(matching: ModelFilter(
    vision: true,
    minimumContextWindow: 200_000
))
```

Catalog provenance and freshness:

```swift
print(catalog.revision.schemaVersion)      // document shape
print(catalog.revision.catalogRevision)    // changes when the data changes
print(catalog.revision.generatedAt as Any) // when it was published
```

Every snippet above is exercised by `ModelSDKExampleCompileTests`, so the
documented API and the shipped API cannot drift apart.

## Behaviour worth knowing

- **One source.** `https://models.lingxifox.cn/models.json`, upstream
  `https://models.dev/api.json`. There is no second catalog to reconcile.
- **A failed fetch is not an empty catalog.** If a previous copy is on hand it is
  returned; only having nothing at all raises `ModelCatalogError`.
- **Unknown means unknown.** `contextWindow`, `pricing.input` and friends stay
  `nil` when the source states nothing, rather than defaulting to zero.
- **Unmodelled fields survive.** The SDK carries what it has never heard of in
  `CatalogModel.fields`, so a new upstream field is reachable without waiting
  for a release of this package.
- **Schema compatibility lives here.** When the catalog moves from schema v2 to
  v3, this package absorbs it; consumers keep the same public API.
- **Ordering is a rule, not an accident.** Providers sort by display name with a
  digit-aware comparison and the id as tie-break; models sort newest-first.
  `ModelCatalogOrdering` is the single comparator, and the published document is
  emitted in the same order, so a list looks the same everywhere.
- **Runtime behaviour is not decided here.** Which wire protocol a vendor
  speaks, how it authenticates, what endpoint it uses — that belongs to the
  consumer's provider layer, and this catalog states none of it.

## Offline use

Decode a document you already hold — a bundled file, a cached copy, a fixture:

```swift
let catalog = try LingXiModelCatalog.decoded(from: Data(contentsOf: url))
```

## Tests

```bash
swift build
swift test
```

## Platforms

Declared in `Package.swift`: macOS 13+, iOS 16+, tvOS 16+, watchOS 9+.

The floor comes from what the code actually uses (URLSession, FileManager,
actors), not from the host product — this package is not macOS-only. macOS,
iOS, tvOS and watchOS were compiled against their SDKs when the repository was
split out. The networking path falls back to the completion-handler form under
`#if canImport(FoundationNetworking)`, so Linux and Windows are source-compatible;
they are not verified from this repository while it has no CI of its own.

## License

MIT — see [LICENSE](LICENSE). Commercial use, third-party agent and app
integration, modification, source and binary redistribution, and use inside
closed-source products are all permitted; the only obligation is keeping the
copyright and permission notice.
