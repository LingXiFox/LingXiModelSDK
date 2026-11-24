package cn.lingxifox.modelsdk;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.time.Duration;
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

public final class CatalogLoader {
    public static final String DEFAULT_ENDPOINT = "https://models.lingxifox.cn/models.json";

    public record Config(
        String endpoint,
        Duration maxAge,
        Path cacheDirectory,
        Duration requestTimeout,
        String userAgent
    ) {
        public static Config defaults() {
            return new Config(DEFAULT_ENDPOINT, Duration.ofMinutes(30), null, Duration.ofSeconds(30), "LingXiModelSDK-Java/2.0");
        }
    }

    public record HttpReply(int statusCode, byte[] body, String etag) {}

    public interface Transport {
        CompletableFuture<HttpReply> fetch(String url, Map<String, String> headers, Duration timeout);
    }

    public static final class JavaHttpTransport implements Transport {
        private final HttpClient client;

        public JavaHttpTransport() {
            this.client = HttpClient.newBuilder().followRedirects(HttpClient.Redirect.NORMAL).build();
        }

        @Override
        public CompletableFuture<HttpReply> fetch(String url, Map<String, String> headers, Duration timeout) {
            HttpRequest.Builder req = HttpRequest.newBuilder(URI.create(url)).timeout(timeout).GET();
            headers.forEach(req::header);
            return client.sendAsync(req.build(), HttpResponse.BodyHandlers.ofByteArray())
                .thenApply(resp -> new HttpReply(
                    resp.statusCode(),
                    resp.body(),
                    resp.headers().firstValue("ETag").orElse(null)
                ));
        }
    }

    public record CachedCatalog(LingXiModelCatalog catalog, Instant fetchedAt, String etag) {
        public boolean isFresh(Duration maxAge, Instant now) {
            return maxAge != null && !maxAge.isZero() && !maxAge.isNegative()
                && Duration.between(fetchedAt, now).compareTo(maxAge) < 0;
        }
    }

    private final Config config;
    private final Transport transport;
    private volatile CachedCatalog memory;

    public CatalogLoader(Config config) {
        this(config, new JavaHttpTransport());
    }

    public CatalogLoader(Config config, Transport transport) {
        this.config = config;
        this.transport = transport;
    }

    public synchronized CachedCatalog cached() {
        if (memory != null) return memory;
        if (config.cacheDirectory() == null) return null;
        Path catPath = config.cacheDirectory().resolve("models.json");
        if (!Files.exists(catPath)) return null;
        try {
            byte[] raw = Files.readAllBytes(catPath);
            LingXiModelCatalog catalog = LingXiModelCatalog.decode(raw);
            String etag = null;
            Instant fetchedAt = catalog.revision().generatedAt() != null
                ? catalog.revision().generatedAt()
                : Instant.EPOCH;
            Path metaPath = config.cacheDirectory().resolve("models.json.meta.json");
            if (Files.exists(metaPath)) {
                Object parsed = JsonParser.parse(Files.readString(metaPath, StandardCharsets.UTF_8));
                if (parsed instanceof Map<?, ?> m) {
                    if (m.get("etag") instanceof String s) etag = s;
                    if (m.get("fetchedAt") instanceof String ts) {
                        try { fetchedAt = Instant.parse(ts); } catch (Exception ignored) {}
                    }
                }
            }
            memory = new CachedCatalog(catalog, fetchedAt, etag);
            return memory;
        } catch (Exception ignored) {
            return null;
        }
    }

    public LingXiModelCatalog load(boolean forceRefresh) {
        try {
            return loadAsync(forceRefresh).join();
        } catch (java.util.concurrent.CompletionException ce) {
            if (ce.getCause() instanceof CatalogException catErr) {
                throw catErr;
            }
            throw ce;
        }
    }

    public CompletableFuture<LingXiModelCatalog> loadAsync(boolean forceRefresh) {
        CachedCatalog cached = cached();
        Instant now = Instant.now();
        if (!forceRefresh && cached != null && cached.isFresh(config.maxAge(), now)) {
            return CompletableFuture.completedFuture(cached.catalog());
        }

        Map<String, String> headers = new LinkedHashMap<>();
        headers.put("Accept", "application/json");
        headers.put("User-Agent", config.userAgent());
        if (cached != null && cached.etag() != null && !cached.etag().isEmpty()) {
            headers.put("If-None-Match", cached.etag());
        }

        return transport.fetch(config.endpoint(), headers, config.requestTimeout())
            .handle((reply, err) -> {
                if (err != null) {
                    if (cached != null) return cached.catalog();
                    Throwable cause = err.getCause() != null ? err.getCause() : err;
                    if (cause instanceof CatalogException ce) throw ce;
                    throw new CatalogException(CatalogException.Kind.TRANSPORT, cause.getMessage(), 0, null, cause);
                }
                if (reply.statusCode() == 304 && cached != null) {
                    String etag = reply.etag() != null ? reply.etag() : cached.etag();
                    Instant ts = Instant.now();
                    memory = new CachedCatalog(cached.catalog(), ts, etag);
                    persistMeta(etag, ts);
                    return cached.catalog();
                }
                if (reply.statusCode() < 200 || reply.statusCode() > 299) {
                    if (cached != null) return cached.catalog();
                    throw new CatalogException(
                        CatalogException.Kind.UNEXPECTED_STATUS,
                        "Unexpected HTTP status: " + reply.statusCode(),
                        reply.statusCode(),
                        null,
                        null
                    );
                }
                try {
                    LingXiModelCatalog cat = LingXiModelCatalog.decode(reply.body());
                    Instant ts = Instant.now();
                    memory = new CachedCatalog(cat, ts, reply.etag());
                    persistCatalog(reply.body(), reply.etag(), ts);
                    return cat;
                } catch (RuntimeException decodeErr) {
                    if (cached != null) return cached.catalog();
                    throw decodeErr;
                }
            });
    }

    private void persistCatalog(byte[] raw, String etag, Instant fetchedAt) {
        if (config.cacheDirectory() == null) return;
        writeAtomic(config.cacheDirectory().resolve("models.json"), raw);
        persistMeta(etag, fetchedAt);
    }

    private void persistMeta(String etag, Instant fetchedAt) {
        if (config.cacheDirectory() == null) return;
        String escapedEtag = etag == null ? "null" : "\"" + etag.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
        String json = "{\"etag\":" + escapedEtag + ",\"fetchedAt\":\"" + fetchedAt.toString() + "\"}";
        writeAtomic(config.cacheDirectory().resolve("models.json.meta.json"), json.getBytes(StandardCharsets.UTF_8));
    }

    private static void writeAtomic(Path target, byte[] data) {
        try {
            Files.createDirectories(target.getParent());
            Path tmp = Files.createTempFile(target.getParent(), ".cache-", ".tmp");
            Files.write(tmp, data);
            Files.move(tmp, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (IOException ignored) {
        }
    }
}
