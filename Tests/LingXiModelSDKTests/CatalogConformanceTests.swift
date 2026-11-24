import Foundation
import Testing

@testable import LingXiModelSDK

struct CatalogConformanceTests {
    private static func conformanceRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("conformance/catalog", isDirectory: true)
    }

    private static func fixtureData(_ name: String) throws -> Data {
        try Data(contentsOf: conformanceRoot().appendingPathComponent("fixtures/\(name)"))
    }

    @Test("conformance v2 catalog decoding, ordering, lookup, filter and three-state semantics")
    func v2Conformance() throws {
        let expData = try Data(contentsOf: Self.conformanceRoot().appendingPathComponent("expectations.json"))
        let exp = try JSONSerialization.jsonObject(with: expData) as! [String: Any]
        let catalog = try LingXiModelCatalog.decoded(from: Self.fixtureData("v2_catalog.json"))

        #expect(catalog.revision.schemaVersion == (exp["schemaVersion"] as! String))
        #expect(catalog.revision.catalogRevision == (exp["catalogRevision"] as! String))
        #expect(catalog.revision.totalProviders == (exp["totalProviders"] as! Int))
        #expect(catalog.totalModels == (exp["totalModels"] as! Int))

        #expect(catalog.sortedProviders().map(\.id) == (exp["providerOrder"] as! [String]))

        let featuredObj = exp["featuredProviderOrder"] as! [String: Any]
        let featured = featuredObj["featured"] as! [String]
        let expectedFeatured = featuredObj["expected"] as! [String]
        #expect(catalog.sortedProviders(featured: featured).map(\.id) == expectedFeatured)

        #expect(catalog.models(provider: "prov10").map(\.id) == (exp["prov10ModelOrder"] as! [String]))
        #expect(catalog.models(provider: "deepseek").map(\.id) == (exp["deepseekModelOrder"] as! [String]))
        #expect(catalog.allModels.map(\.qualifiedID) == (exp["allModelQualifiedOrder"] as! [String]))

        // Three-state semantics & unknown fields
        let gpt10 = try #require(catalog.model(provider: "PROV10", id: "GPT-10"))
        #expect(gpt10.pricing.input == 0.0)
        #expect(gpt10.capabilities.supportsTemperature == false)
        #expect(gpt10.capabilities.vision == true)
        #expect(gpt10.capabilities.audioInput == true)
        #expect(gpt10.capabilities.audioOutput == true)
        #expect(gpt10.capabilities.reasoningEfforts == ["low", "medium", "high"])
        #expect(gpt10.fields["experimental"]?.objectValue?["warmup"]?.boolValue == true)

        let gpt5 = try #require(catalog.model(id: "prov10/gpt-5"))
        #expect(gpt5.contextWindow == 0)
        #expect(gpt5.pricing.input == nil)
        #expect(gpt5.pricing.isUnknown == true)
        #expect(gpt5.capabilities.supportsTemperature == nil)
        #expect(gpt5.capabilities.structuredOutput == false)

        let prov10 = try #require(catalog.provider("prov10"))
        #expect(prov10.fields["customProviderField"]?.objectValue?["tier"]?.stringValue == "standard")

        // Search & Filters
        let searchExp = exp["search"] as! [String: [String]]
        #expect(catalog.search("gpt", limit: 1).map(\.qualifiedID) == searchExp["gptLimit1"]!)
        #expect(catalog.search("deepseek").map(\.qualifiedID) == searchExp["deepseek"]!)

        let filterExp = exp["filters"] as! [String: [String]]
        #expect(catalog.models(matching: ModelFilter(vision: true)).map(\.qualifiedID) == filterExp["vision"]!)
        #expect(catalog.models(matching: ModelFilter(minimumContextWindow: 200_000)).map(\.qualifiedID) == filterExp["minContext200k"]!)
        #expect(catalog.models(matching: ModelFilter(providerID: "deepseek")).map(\.qualifiedID) == filterExp["deepseekDefault"]!)
        #expect(catalog.models(matching: ModelFilter(providerID: "deepseek", includeDeprecated: true)).map(\.qualifiedID) == filterExp["deepseekIncludeDeprecated"]!)
    }

    @Test("conformance v1 legacy, malformed, unsupported schema, and broken fixtures")
    func errorAndCompatibilityFixtures() throws {
        let legacy = try LingXiModelCatalog.decoded(from: Self.fixtureData("v1_legacy.json"))
        #expect(legacy.revision.schemaVersion == "1.0")
        #expect(legacy.model(provider: "ollama", id: "llama3")?.contextWindow == 8192)

        let malformed = try LingXiModelCatalog.decoded(from: Self.fixtureData("malformed_entry.json"))
        #expect(malformed.provider("p")?.models.map(\.id) == ["published-key"])
        #expect(malformed.model(provider: "p", id: "internal-id") == nil)

        #expect(throws: ModelCatalogError.unsupportedSchemaVersion("3.0")) {
            _ = try LingXiModelCatalog.decoded(from: Self.fixtureData("unsupported_schema.json"))
        }

        #expect(throws: ModelCatalogError.self) {
            _ = try LingXiModelCatalog.decoded(from: Self.fixtureData("broken_empty_providers.json"))
        }
    }
}
