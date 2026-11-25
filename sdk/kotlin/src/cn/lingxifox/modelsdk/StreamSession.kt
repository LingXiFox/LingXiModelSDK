package cn.lingxifox.modelsdk

import java.util.concurrent.atomic.AtomicBoolean

/**
 * Incremental, consumer-driven view over a core-owned stream.
 *
 * Mirrors the Rust core's pull model: [next] blocks until the core yields the
 * next normalized event, [collect] drains the stream, and [close] is idempotent
 * and safe after [cancel].
 */
class StreamSession private constructor(
    private val bridge: KotlinBridge,
    private val handle: Long,
    val unified: Boolean,
) : AutoCloseable {
    private val closed = AtomicBoolean(false)

    /** Returns the next event, or null when the stream is exhausted. */
    fun next(): Map<String, Any?>? = next(DEFAULT_TIMEOUT_MS)

    /** Returns the next event, or null when the stream is exhausted. */
    fun next(timeoutMs: Long): Map<String, Any?>? {
        check(!closed.get()) { "stream session is closed" }
        val envelope = bridge.streamNext(handle, timeoutMs)
        if (envelope["ok"] != true) {
            val code = envelope["code"]?.toString()
            val error = envelope["error"]?.toString() ?: "stream failed"
            throw LingXiCoreException("stream failed" + (code?.let { " [$it]" } ?: "") + ": $error", code, null)
        }
        if (envelope["done"] == true) return null
        @Suppress("UNCHECKED_CAST")
        return envelope["event"] as? Map<String, Any?>
            ?: throw LingXiCoreException("stream_next returned no event")
    }

    /** Drains the stream into a list. */
    fun collect(): List<Map<String, Any?>> {
        val events = ArrayList<Map<String, Any?>>()
        while (true) {
            events.add(next() ?: break)
        }
        return events
    }

    fun cancel() {
        if (closed.get()) return
        bridge.streamCancel(handle)
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        bridge.streamClose(handle)
    }

    companion object {
        const val DEFAULT_TIMEOUT_MS: Long = 60_000L

        /** Opens a stream; [request] carries protocol/baseUrl/apiKey plus payload or request. */
        fun open(request: Map<String, Any?>, unified: Boolean): StreamSession {
            val bridge = KotlinBridge.shared()
            val openRequest = LinkedHashMap<String, Any?>(request)
            openRequest["unified"] = unified
            val handle = bridge.streamOpen(openRequest)
            if (handle == 0L) {
                throw LingXiCoreException("stream_open failed: " + bridge.streamOpenError())
            }
            return StreamSession(bridge, handle, unified)
        }
    }
}
