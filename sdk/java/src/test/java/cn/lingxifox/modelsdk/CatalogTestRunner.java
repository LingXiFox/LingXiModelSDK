package cn.lingxifox.modelsdk;

import cn.lingxifox.modelsdk.CatalogTypes.CatalogModel;
import cn.lingxifox.modelsdk.CatalogTypes.CatalogProvider;
import cn.lingxifox.modelsdk.CatalogTypes.ModelFilter;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;

public final class CatalogTestRunner {
    private static Path conformanceDir() {
        return Path.of("..", "..", "conformance", "catalog").toAbsolutePath().normalize();
    }

    private static byte[] readFixture(String name) throws IOException {
        return Files.readAllBytes(conformanceDir().resolve("fixtures").resolve(name));
    }

    private static void assertEq(Object actual, Object expected, String msg) {
        if (actual == null && expected == null) return;
        if (actual != null && actual.equals(expected)) return;
        throw new AssertionError(msg + ": got " + actual + ", expected " + expected);
    }

    private static void assertTrue(boolean cond, String msg) {
        if (!cond) throw new AssertionError(msg);
    }

    @SuppressWarnings("unchecked")
    private static void testConformance() throws Exception {
        Map<String, Object> exp = (Map<String, Object>) JsonParser.parse(
            Files.readString(conformanceDir().resolve("expectations.json"), StandardCharsets.UTF_8)
        );
        LingXiModelCatalog catalog = LingXiModelCatalog.decode(readFixture("v2_catalog.json"));

        assertEq(catalog.revision().schemaVersion(), exp.get("schemaVersion"), "schemaVersion");
        assertEq(catalog.revision().catalogRevision(), exp.get("catalogRevision"), "catalogRevision");
        assertEq((long) catalog.revision().totalProviders(), exp.get("totalProviders"), "totalProviders");
        assertEq((long) catalog.totalModels(), exp.get("totalModels"), "totalModels");

        assertEq(
            catalog.sortedProviders(List.of()).stream().map(CatalogProvider::id).toList(),
            exp.get("providerOrder"),
            "providerOrder"
        );
        Map<String, Object> feat = (Map<String, Object>) exp.get("featuredProviderOrder");
        assertEq(
            catalog.sortedProviders((List<String>) feat.get("featured")).stream().map(CatalogProvider::id).toList(),
            feat.get("expected"),
            "featuredProviderOrder"
        );
        assertEq(
            catalog.modelsForProvider("prov10").stream().map(CatalogModel::id).toList(),
            exp.get("prov10ModelOrder"),
            "prov10ModelOrder"
        );
        assertEq(
            catalog.modelsForProvider("deepseek").stream().map(CatalogModel::id).toList(),
            exp.get("deepseekModelOrder"),
            "deepseekModelOrder"
        );
        assertEq(
            catalog.allModels().stream().map(CatalogModel::qualifiedID).toList(),
            exp.get("allModelQualifiedOrder"),
            "allModelQualifiedOrder"
        );

        CatalogModel gpt10 = catalog.model("PROV10", "GPT-10").orElseThrow();
        assertEq(gpt10.pricing().input(), 0.0, "gpt10 zero input price");
        assertTrue(!gpt10.pricing().isUnknown(), "gpt10 pricing known");
        assertEq(gpt10.capabilities().supportsTemperature(), Boolean.FALSE, "gpt10 temperature=false");
        assertTrue(gpt10.capabilities().vision(), "gpt10 vision");
        assertTrue(gpt10.capabilities().audioInput(), "gpt10 audioInput");
        assertTrue(gpt10.capabilities().audioOutput(), "gpt10 audioOutput");
        assertEq(gpt10.capabilities().reasoningEfforts(), List.of("low", "medium", "high"), "gpt10 efforts");
        Map<String, Object> expMap = (Map<String, Object>) gpt10.fields().get("experimental");
        assertEq(expMap.get("warmup"), Boolean.TRUE, "unknown experimental.warmup");

        CatalogModel gpt5 = catalog.model("prov10/gpt-5").orElseThrow();
        assertEq(gpt5.contextWindow(), 0L, "gpt5 context=0");
        assertTrue(gpt5.pricing().isUnknown(), "gpt5 unknown pricing");
        assertEq(gpt5.capabilities().supportsTemperature(), null, "gpt5 nil temperature");
        assertTrue(!gpt5.capabilities().structuredOutput(), "gpt5 structured_output=false");

        Map<String, Object> searchExp = (Map<String, Object>) exp.get("search");
        assertEq(catalog.search("gpt", 1).stream().map(CatalogModel::qualifiedID).toList(), searchExp.get("gptLimit1"), "search gpt");
        assertEq(catalog.search("deepseek", null).stream().map(CatalogModel::qualifiedID).toList(), searchExp.get("deepseek"), "search deepseek");

        Map<String, Object> filterExp = (Map<String, Object>) exp.get("filters");
        assertEq(catalog.modelsMatching(ModelFilter.builder().vision(true).build()).stream().map(CatalogModel::qualifiedID).toList(), filterExp.get("vision"), "filter vision");
        assertEq(catalog.modelsMatching(ModelFilter.builder().minimumContextWindow(200_000L).build()).stream().map(CatalogModel::qualifiedID).toList(), filterExp.get("minContext200k"), "filter 200k");
        assertEq(catalog.modelsMatching(ModelFilter.builder().providerID("deepseek").build()).stream().map(CatalogModel::qualifiedID).toList(), filterExp.get("deepseekDefault"), "filter ds default");
        assertEq(catalog.modelsMatching(ModelFilter.builder().providerID("deepseek").includeDeprecated(true).build()).stream().map(CatalogModel::qualifiedID).toList(), filterExp.get("deepseekIncludeDeprecated"), "filter ds deprecated");

        LingXiModelCatalog legacy = LingXiModelCatalog.decode(readFixture("v1_legacy.json"));
        assertEq(legacy.revision().schemaVersion(), "1.0", "legacy version");
        assertEq(legacy.model("ollama", "llama3").orElseThrow().contextWindow(), 8192L, "legacy context");

        LingXiModelCatalog malformed = LingXiModelCatalog.decode(readFixture("malformed_entry.json"));
        assertTrue(malformed.model("p", "published-key").isPresent(), "published-key present");
        assertTrue(malformed.model("p", "internal-id").isEmpty(), "internal-id absent");

        try {
            LingXiModelCatalog.decode(readFixture("unsupported_schema.json"));
            throw new AssertionError("expected unsupported schema error");
        } catch (CatalogException ce) {
            assertEq(ce.getKind(), CatalogException.Kind.UNSUPPORTED_SCHEMA_VERSION, "unsupported schema kind");
        }

        try {
            LingXiModelCatalog.decode(readFixture("broken_empty_providers.json"));
            throw new AssertionError("expected invalid catalog error");
        } catch (CatalogException ce) {
            assertEq(ce.getKind(), CatalogException.Kind.INVALID_CATALOG, "empty providers kind");
        }
    }

    private static void testLoader() throws Exception {
        byte[] fixture = readFixture("v2_catalog.json");
        AtomicInteger mode = new AtomicInteger(0);
        AtomicInteger hits = new AtomicInteger(0);

        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/models.json", exchange -> {
            hits.incrementAndGet();
            if (mode.get() == 0) {
                String inm = exchange.getRequestHeaders().getFirst("If-None-Match");
                if ("\"rev-1\"".equals(inm)) {
                    exchange.sendResponseHeaders(304, -1);
                } else {
                    exchange.getResponseHeaders().add("ETag", "\"rev-1\"");
                    exchange.sendResponseHeaders(200, fixture.length);
                    exchange.getResponseBody().write(fixture);
                }
            } else {
                exchange.sendResponseHeaders(503, -1);
            }
            exchange.close();
        });
        server.start();
        Path cacheDir = Files.createTempDirectory("lingxi-java-cache-");
        try {
            String url = "http://127.0.0.1:" + server.getAddress().getPort() + "/models.json";
            CatalogLoader loader = new CatalogLoader(
                new CatalogLoader.Config(url, Duration.ofMinutes(30), cacheDir, Duration.ofSeconds(10), "Test/1.0")
            );
            LingXiModelCatalog cat1 = loader.load(false);
            assertEq(cat1.revision().catalogRevision(), "conf-rev-001", "loader rev");
            loader.load(false);
            assertEq(hits.get(), 1, "fresh cache skipped network");

            loader.load(true);
            assertEq(hits.get(), 2, "forceRefresh hit 304");

            mode.set(1);
            CatalogLoader cold = new CatalogLoader(
                new CatalogLoader.Config(url, Duration.ZERO, cacheDir, Duration.ofSeconds(10), "Test/1.0")
            );
            LingXiModelCatalog fallback = cold.load(false);
            assertEq(fallback.totalModels(), 5, "disk fallback models");

            CatalogLoader empty = new CatalogLoader(
                new CatalogLoader.Config(url, Duration.ZERO, null, Duration.ofSeconds(10), "Test/1.0")
            );
            try {
                empty.load(false);
                throw new AssertionError("expected 503 failure");
            } catch (CatalogException ce) {
                assertEq(ce.getKind(), CatalogException.Kind.UNEXPECTED_STATUS, "503 kind");
                assertEq(ce.getStatusCode(), 503, "503 status");
            }
        } finally {
            server.stop(0);
            Files.walk(cacheDir).sorted((a, b) -> b.compareTo(a)).forEach(p -> {
                try { Files.deleteIfExists(p); } catch (IOException ignored) {}
            });
        }
    }

    public static void main(String[] args) throws Exception {
        testConformance();
        testLoader();
        System.out.println("Java Catalog SDK conformance + loader tests passed.");
    }
}
