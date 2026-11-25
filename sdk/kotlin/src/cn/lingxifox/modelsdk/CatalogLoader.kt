package cn.lingxifox.modelsdk

import java.io.IOException
import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse
import java.nio.charset.StandardCharsets
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.time.Duration
import java.time.Instant
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CompletionException

/**
 * Fetches and caches the LingXi model catalog.
 *
 * Behaviour: honour `maxAge` in memory, send `If-None-Match` with the cached
 * ETag, treat 304 as "unchanged", persist to `models.json` + `models.json.meta.json`,
 * and fall back to the stale cache when the network fails.
 */
class CatalogLoader
@JvmOverloads
constructor(
    val config: Config,
    private val transport: Transport = JavaHttpTransport(),
) {
    data class Config(
        val endpoint: String,
        val maxAge: Duration?,
        val cacheDirectory: Path?,
        val requestTimeout: Duration,
        val userAgent: String,
    ) {
        companion object {
            fun defaults(): Config = Config(
                endpoint = DEFAULT_ENDPOINT,
                maxAge = Duration.ofMinutes(30),
                cacheDirectory = null,
                requestTimeout = Duration.ofSeconds(30),
                userAgent = "LingXiModelSDK-Kotlin/2.0",
            )
        }
    }

    data class HttpReply(val statusCode: Int, val body: ByteArray, val etag: String?) {
        override fun equals(other: Any?): Boolean = this === other
        override fun hashCode(): Int = System.identityHashCode(this)
    }

    fun interface Transport {
        fun fetch(url: String, headers: Map<String, String>, timeout: Duration): CompletableFuture<HttpReply>
    }

    class JavaHttpTransport : Transport {
        private val client: HttpClient = HttpClient.newBuilder()
            .followRedirects(HttpClient.Redirect.NORMAL)
            .build()

        override fun fetch(
            url: String,
            headers: Map<String, String>,
            timeout: Duration,
        ): CompletableFuture<HttpReply> {
            val builder = HttpRequest.newBuilder(URI.create(url)).timeout(timeout).GET()
            headers.forEach { (key, value) -> builder.header(key, value) }
            return client.sendAsync(builder.build(), HttpResponse.BodyHandlers.ofByteArray())
                .thenApply { response ->
                    HttpReply(response.statusCode(), response.body(), response.headers().firstValue("ETag").orElse(null))
                }
        }
    }

    data class CachedCatalog(val catalog: LingXiModelCatalog, val fetchedAt: Instant, val etag: String?) {
        fun isFresh(maxAge: Duration?, now: Instant): Boolean {
            if (maxAge == null || maxAge.isZero || maxAge.isNegative) return false
            return Duration.between(fetchedAt, now) < maxAge
        }
    }

    @Volatile
    private var memory: CachedCatalog? = null

    /** Returns the cached catalog (memory, then disk), or null when none exists. */
    @Synchronized
    fun cached(): CachedCatalog? {
        memory?.let { return it }
        val directory = config.cacheDirectory ?: return null
        val catalogPath = directory.resolve("models.json")
        if (!Files.exists(catalogPath)) return null
        return try {
            val raw = Files.readAllBytes(catalogPath)
            val catalog = LingXiModelCatalog.decode(raw)
            var etag: String? = null
            var fetchedAt: Instant = catalog.revision.generatedAt ?: Instant.EPOCH
            val metaPath = directory.resolve("models.json.meta.json")
            if (Files.exists(metaPath)) {
                val parsed = Json.parse(Files.readString(metaPath, StandardCharsets.UTF_8))
                if (parsed is Map<*, *>) {
                    (parsed["etag"] as? String)?.let { etag = it }
                    (parsed["fetchedAt"] as? String)?.let { text ->
                        try {
                            fetchedAt = Instant.parse(text)
                        } catch (_: Exception) {
                            // Keep the catalog's own timestamp.
                        }
                    }
                }
            }
            CachedCatalog(catalog, fetchedAt, etag).also { memory = it }
        } catch (_: Exception) {
            null
        }
    }

    fun load(forceRefresh: Boolean): LingXiModelCatalog = try {
        loadAsync(forceRefresh).join()
    } catch (e: CompletionException) {
        if (e.cause is CatalogException) throw e.cause as CatalogException
        throw e
    }

    fun loadAsync(forceRefresh: Boolean): CompletableFuture<LingXiModelCatalog> {
        val cached = cached()
        val now = Instant.now()
        if (!forceRefresh && cached != null && cached.isFresh(config.maxAge, now)) {
            return CompletableFuture.completedFuture(cached.catalog)
        }

        val headers = LinkedHashMap<String, String>()
        headers["Accept"] = "application/json"
        headers["User-Agent"] = config.userAgent
        val cachedEtag = cached?.etag
        if (!cachedEtag.isNullOrEmpty()) headers["If-None-Match"] = cachedEtag

        return transport.fetch(config.endpoint, headers, config.requestTimeout).handle { reply, error ->
            if (error != null) {
                if (cached != null) return@handle cached.catalog
                val cause = error.cause ?: error
                if (cause is CatalogException) throw cause
                throw CatalogException(
                    CatalogException.Kind.TRANSPORT,
                    cause.message ?: cause.toString(),
                    0,
                    null,
                    cause,
                )
            }
            if (reply.statusCode == 304 && cached != null) {
                val etag = reply.etag ?: cached.etag
                val timestamp = Instant.now()
                memory = CachedCatalog(cached.catalog, timestamp, etag)
                persistMeta(etag, timestamp)
                return@handle cached.catalog
            }
            if (reply.statusCode !in 200..299) {
                if (cached != null) return@handle cached.catalog
                throw CatalogException(
                    CatalogException.Kind.UNEXPECTED_STATUS,
                    "Unexpected HTTP status: ${reply.statusCode}",
                    reply.statusCode,
                    null,
                    null,
                )
            }
            try {
                val catalog = LingXiModelCatalog.decode(reply.body)
                val timestamp = Instant.now()
                memory = CachedCatalog(catalog, timestamp, reply.etag)
                persistCatalog(reply.body, reply.etag, timestamp)
                catalog
            } catch (decodeError: RuntimeException) {
                if (cached != null) cached.catalog else throw decodeError
            }
        }
    }

    private fun persistCatalog(raw: ByteArray, etag: String?, fetchedAt: Instant) {
        val directory = config.cacheDirectory ?: return
        writeAtomic(directory.resolve("models.json"), raw)
        persistMeta(etag, fetchedAt)
    }

    private fun persistMeta(etag: String?, fetchedAt: Instant) {
        val directory = config.cacheDirectory ?: return
        val payload = linkedMapOf<String, Any?>("etag" to etag, "fetchedAt" to fetchedAt.toString())
        writeAtomic(directory.resolve("models.json.meta.json"), Json.write(payload).toByteArray(StandardCharsets.UTF_8))
    }

    private fun writeAtomic(target: Path, data: ByteArray) {
        try {
            val parent = target.parent ?: return
            Files.createDirectories(parent)
            val tmp = Files.createTempFile(parent, ".cache-", ".tmp")
            Files.write(tmp, data)
            Files.move(tmp, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE)
        } catch (_: IOException) {
            // Caching is best effort; a read-only cache must not fail the load.
        }
    }

    companion object {
        const val DEFAULT_ENDPOINT: String = "https://models.lingxifox.cn/models.json"
    }
}
