package cn.lingxifox.modelsdk

import java.lang.foreign.Arena
import java.lang.foreign.FunctionDescriptor
import java.lang.foreign.Linker
import java.lang.foreign.MemorySegment
import java.lang.foreign.SymbolLookup
import java.lang.foreign.ValueLayout
import java.lang.invoke.MethodHandle
import java.nio.file.Files
import java.nio.file.Path

/**
 * Foreign-function bridge to the Rust `lingxi_core` library.
 *
 * All LingXi protocol knowledge lives in the Rust core; this class only moves
 * JSON across the C ABI. Requires JDK 22+ (final Foreign Function & Memory API,
 * `--enable-native-access=ALL-UNNAMED`) and the shared library produced by
 * `cargo build` in `core/`.
 *
 * The library is located from `LINGXI_CORE_LIB`, the `lingxi.core.lib` system
 * property, or a `core/target/{release,debug}` directory walking up from the
 * working directory.
 */
class KotlinBridge private constructor(val libraryPath: Path) {
    private val arena: Arena = Arena.ofShared()
    private val translateRequest: MethodHandle
    private val normalizeResponse: MethodHandle
    private val normalizeStreamEvents: MethodHandle
    private val stringFree: MethodHandle
    private val streamOpen: MethodHandle
    private val streamOpenErrorHandle: MethodHandle
    private val streamNextHandle: MethodHandle
    private val streamCancelHandle: MethodHandle
    private val streamCloseHandle: MethodHandle
    private val streamLastErrorHandle: MethodHandle
    private val nativePrepareHandle: MethodHandle
    private val unifiedPrepareHandle: MethodHandle
    private val unifiedEmbedHandle: MethodHandle
    private val unifiedRerankHandle: MethodHandle

    init {
        val linker = Linker.nativeLinker()
        val lookup = SymbolLookup.libraryLookup(libraryPath, arena)
        fun downcall(name: String, descriptor: FunctionDescriptor): MethodHandle {
            val symbol = lookup.find(name).orElseThrow {
                IllegalStateException(
                    "lingxi_core is missing $name; rebuild it with: cargo build --manifest-path core/Cargo.toml"
                )
            }
            return linker.downcallHandle(symbol, descriptor)
        }

        translateRequest = downcall(
            "lingxi_lmp_translate_request",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        normalizeResponse = downcall(
            "lingxi_lmp_normalize_response",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        normalizeStreamEvents = downcall(
            "lingxi_lmp_normalize_stream_events",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        nativePrepareHandle = downcall(
            "lingxi_native_prepare",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        unifiedPrepareHandle = downcall(
            "lingxi_unified_prepare",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        unifiedEmbedHandle = downcall(
            "lingxi_unified_embed",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        unifiedRerankHandle = downcall(
            "lingxi_unified_rerank",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
        stringFree = downcall("lingxi_string_free", FunctionDescriptor.ofVoid(ValueLayout.ADDRESS))

        streamOpen = downcall("lingxi_stream_open", FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS))
        streamOpenErrorHandle = downcall("lingxi_stream_open_error", FunctionDescriptor.of(ValueLayout.ADDRESS))
        streamNextHandle = downcall(
            "lingxi_stream_next",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG),
        )
        streamCancelHandle = downcall("lingxi_stream_cancel", FunctionDescriptor.ofVoid(ValueLayout.ADDRESS))
        streamCloseHandle = downcall("lingxi_stream_close", FunctionDescriptor.ofVoid(ValueLayout.ADDRESS))
        streamLastErrorHandle = downcall(
            "lingxi_stream_last_error",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS),
        )
    }

    // ---- JSON marshalling -------------------------------------------------


    private fun callJson(handle: MethodHandle, vararg args: String): Map<String, Any?> {
        try {
            Arena.ofConfined().use { call ->
                val segments = args.map { call.allocateFrom(it) }
                val ptr = handle.invokeWithArguments(*segments.toTypedArray()) as MemorySegment
                if (ptr.address() == 0L) {
                    throw LingXiCoreException("lingxi_core returned a null result")
                }
                val json = try {
                    ptr.reinterpret(Long.MAX_VALUE).getString(0)
                } finally {
                    free(ptr)
                }
                @Suppress("UNCHECKED_CAST")
                return Json.parse(json) as? Map<String, Any?>
                    ?: throw LingXiCoreException("lingxi_core returned non-object JSON: $json")
            }
        } catch (e: LingXiCoreException) {
            throw e
        } catch (e: Throwable) {
            throw LingXiCoreException("lingxi_core call failed: $e", null, e)
        }
    }

    private fun callJsonAddr(handle: MethodHandle, addr: Long): Map<String, Any?> {
        try {
            val ptr = handle.invokeWithArguments(MemorySegment.ofAddress(addr)) as MemorySegment
            if (ptr.address() == 0L) return mapOf("ok" to false, "error" to "null result")
            val json = try {
                ptr.reinterpret(Long.MAX_VALUE).getString(0)
            } finally {
                free(ptr)
            }
            @Suppress("UNCHECKED_CAST")
            return Json.parse(json) as? Map<String, Any?>
                ?: throw LingXiCoreException("lingxi_core returned non-object JSON: $json")
        } catch (e: LingXiCoreException) {
            throw e
        } catch (e: Throwable) {
            throw LingXiCoreException("lingxi_core call failed: $e", null, e)
        }
    }

    private fun free(ptr: MemorySegment) {
        try {
            stringFree.invokeExact(ptr)
        } catch (_: Throwable) {
            // Freeing is best effort; a leak is preferable to masking a real error.
        }
    }

    private fun requireOk(envelope: Map<String, Any?>, operation: String): Map<String, Any?> {
        if (envelope["ok"] == true) return envelope
        val code = envelope["code"]
        val error = envelope["error"]
        throw LingXiCoreException(
            "$operation failed" + (code?.let { " [$it]" } ?: "") + ": " + (error ?: "unknown error"),
            code?.toString(),
            null,
        )
    }

    private fun mapOfEnvelope(value: Any?, key: String, operation: String): Map<String, Any?> {
        @Suppress("UNCHECKED_CAST")
        return value as? Map<String, Any?>
            ?: throw LingXiCoreException("$operation returned no '$key' object")
    }

    // ---- LMP translation --------------------------------------------------

    fun translateRequest(protocol: String, request: Map<String, Any?>): Map<String, Any?> {
        val out = requireOk(callJson(translateRequest, protocol, Json.write(request)), "translate_request")
        return mapOfEnvelope(out["payload"], "payload", "translate_request")
    }

    fun normalizeResponse(protocol: String, raw: Map<String, Any?>): Map<String, Any?> {
        val out = requireOk(callJson(normalizeResponse, protocol, Json.write(raw)), "normalize_response")
        return mapOfEnvelope(out["response"], "response", "normalize_response")
    }

    fun normalizeStreamEvents(protocol: String, events: List<Map<String, Any?>>): List<Map<String, Any?>> {
        val out = requireOk(callJson(normalizeStreamEvents, protocol, Json.write(events)), "normalize_stream_events")
        @Suppress("UNCHECKED_CAST")
        return (out["events"] as? List<Map<String, Any?>>) ?: emptyList()
    }

    /** Resolved wire target for a provider-native payload: {url, headers, body, framing}. */
    fun prepareNative(request: Map<String, Any?>): Map<String, Any?> =
        requireOk(callJson(nativePrepareHandle, Json.write(request)), "native_prepare")

    /** Same, but the request is an LMP request the core translates first. */
    fun prepareUnified(request: Map<String, Any?>): Map<String, Any?> =
        requireOk(callJson(unifiedPrepareHandle, Json.write(request)), "unified_prepare")

    fun embed(request: Map<String, Any?>): Map<String, Any?> {
        val out = requireOk(callJson(unifiedEmbedHandle, Json.write(request)), "unified_embed")
        return mapOfEnvelope(out["response"], "response", "unified_embed")
    }

    fun rerank(request: Map<String, Any?>): Map<String, Any?> {
        val out = requireOk(callJson(unifiedRerankHandle, Json.write(request)), "unified_rerank")
        return mapOfEnvelope(out["response"], "response", "unified_rerank")
    }

    // ---- Incremental streams ----------------------------------------------

    /** Opens a core-owned stream. Returns 0 on failure; see [streamOpenError]. */
    fun streamOpen(request: Map<String, Any?>): Long {
        try {
            Arena.ofConfined().use { call ->
                val arg = call.allocateFrom(Json.write(request))
                val ptr = streamOpen.invokeWithArguments(arg) as MemorySegment
                return ptr.address()
            }
        } catch (e: Throwable) {
            throw LingXiCoreException("stream_open failed: $e", null, e)
        }
    }

    fun streamOpenError(): String {
        val envelope = callJson(streamOpenErrorHandle)
        return envelope["error"]?.toString() ?: "unknown stream error"
    }

    fun streamNext(handle: Long, timeoutMs: Long): Map<String, Any?> {
        try {
            val ptr = streamNextHandle.invokeWithArguments(
                MemorySegment.ofAddress(handle),
                timeoutMs,
            ) as MemorySegment
            if (ptr.address() == 0L) {
                return mapOf("ok" to false, "code" to "error", "error" to "null result")
            }
            val json = try {
                ptr.reinterpret(Long.MAX_VALUE).getString(0)
            } finally {
                free(ptr)
            }
            @Suppress("UNCHECKED_CAST")
            return Json.parse(json) as? Map<String, Any?>
                ?: throw LingXiCoreException("stream_next returned non-object JSON: $json")
        } catch (e: LingXiCoreException) {
            throw e
        } catch (e: Throwable) {
            throw LingXiCoreException("stream_next failed: $e", null, e)
        }
    }

    fun streamCancel(handle: Long) {
        try {
            streamCancelHandle.invokeExact(MemorySegment.ofAddress(handle))
        } catch (e: Throwable) {
            throw LingXiCoreException("stream_cancel failed: $e", null, e)
        }
    }

    fun streamClose(handle: Long) {
        try {
            streamCloseHandle.invokeExact(MemorySegment.ofAddress(handle))
        } catch (_: Throwable) {
            // Closing must never throw: the handle may already be gone.
        }
    }

    fun streamLastError(handle: Long): Map<String, Any?> = callJsonAddr(streamLastErrorHandle, handle)

    companion object {
        @Volatile
        private var shared: KotlinBridge? = null

        @Volatile
        private var loadFailure: String? = null

        /** Returns the shared bridge, throwing a clear error when it cannot load. */
        @Synchronized
        fun shared(): KotlinBridge {
            shared?.let { return it }
            loadFailure?.let { throw IllegalStateException(it) }
            val path = locateLibrary()
            if (path == null) {
                loadFailure = "lingxi_core shared library not found; build it with " +
                    "'cargo build --manifest-path core/Cargo.toml' or set LINGXI_CORE_LIB"
                throw IllegalStateException(loadFailure)
            }
            return try {
                val bridge = KotlinBridge(path)
                shared = bridge
                bridge
            } catch (e: Throwable) {
                loadFailure = "could not load $path: $e"
                throw IllegalStateException(loadFailure, e)
            }
        }

        /** True when the Rust core can be loaded. */
        @Synchronized
        fun available(): Boolean = try {
            shared()
            true
        } catch (_: RuntimeException) {
            false
        }

        internal fun locateLibrary(): Path? {
            val env = System.getenv("LINGXI_CORE_LIB")
            if (!env.isNullOrEmpty() && Files.isRegularFile(Path.of(env))) return Path.of(env)
            val prop = System.getProperty("lingxi.core.lib")
            if (!prop.isNullOrEmpty() && Files.isRegularFile(Path.of(prop))) return Path.of(prop)
            val os = System.getProperty("os.name", "").lowercase()
            val name = when {
                os.contains("mac") -> "liblingxi_core.dylib"
                os.contains("win") -> "lingxi_core.dll"
                else -> "liblingxi_core.so"
            }
            var dir = Path.of("").toAbsolutePath()
            while (true) {
                for (profile in arrayOf("release", "debug")) {
                    val candidate = dir.resolve("core/target/$profile/$name")
                    if (Files.isRegularFile(candidate)) return candidate
                }
                dir = dir.parent ?: return null
            }
        }
    }
}
