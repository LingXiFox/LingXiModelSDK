package cn.lingxifox.modelsdk;

import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicBoolean;

/**
 * One open upstream stream, owned by the Rust core.
 *
 * <p>{@link #next()} blocks for exactly one event, so callers observe tokens
 * while the provider response is still open. Unified sessions yield LMP events
 * ({@code text_delta}, {@code tool_call_delta}, ...); native sessions yield
 * {@code {event, data, id}} frames. The transport lives in Rust, so no SSE or
 * NDJSON parsing happens in Java.
 *
 * <p>Call {@code next()} from a background thread, never from the UI thread.
 * {@link #close()} is idempotent and {@link #cancel()} is safe from any thread.
 */
public final class StreamSession implements AutoCloseable {
    private static final long DEFAULT_TIMEOUT_MS = 60_000;

    private final RustCoreBridge bridge;
    private final long handle;
    private final boolean unified;
    private final AtomicBoolean closed = new AtomicBoolean();

    private StreamSession(RustCoreBridge bridge, long handle, boolean unified) {
        this.bridge = bridge;
        this.handle = handle;
        this.unified = unified;
    }

    /**
     * Opens a stream. {@code request} carries protocol, baseUrl, apiKey, model,
     * payload (LMP when {@code unified}, provider-native otherwise) and an
     * optional idleTimeoutMs.
     */
    public static StreamSession open(Map<String, Object> request, boolean unified) {
        RustCoreBridge bridge = RustCoreBridge.shared();
        Map<String, Object> body = new java.util.LinkedHashMap<>(request);
        body.put("unified", unified);
        long handle = bridge.streamOpen(body);
        if (handle == 0) {
            throw new LingXiCoreException("stream_open failed: " + bridge.streamOpenError());
        }
        return new StreamSession(bridge, handle, unified);
    }

    /** True when the session emits LMP events instead of native frames. */
    public boolean unified() {
        return unified;
    }

    /** Waits up to the default timeout for one event; {@code null} means clean end. */
    public Map<String, Object> next() {
        return next(DEFAULT_TIMEOUT_MS);
    }

    /**
     * Waits up to {@code timeoutMs} (0 = forever) for one event.
     * Returns the event, or {@code null} once the stream has ended cleanly.
     * A terminal upstream failure throws {@link LingXiCoreException}.
     */
    public Map<String, Object> next(long timeoutMs) {
        if (closed.get()) {
            throw new IllegalStateException("stream is closed");
        }
        Map<String, Object> envelope = bridge.streamNext(handle, timeoutMs);
        if (Boolean.TRUE.equals(envelope.get("ok"))) {
            if (Boolean.TRUE.equals(envelope.get("done"))) {
                return null;
            }
            Object event = envelope.get("event");
            @SuppressWarnings("unchecked")
            Map<String, Object> map = (Map<String, Object>) event;
            return map;
        }
        Object code = envelope.get("code");
        Object error = envelope.get("error");
        throw new LingXiCoreException("stream failed"
            + (code == null ? "" : " [" + code + "]") + ": " + error,
            code == null ? null : String.valueOf(code), null);
    }

    /** Reads every remaining event; only use this when buffering is acceptable. */
    public List<Map<String, Object>> collect() {
        List<Map<String, Object>> events = new java.util.ArrayList<>();
        Map<String, Object> event;
        while ((event = next()) != null) {
            events.add(event);
        }
        return events;
    }

    /** Asks the blocked reader to stop; safe from any thread. */
    public void cancel() {
        bridge.streamCancel(handle);
    }

    /** Cancels and releases the handle; idempotent. */
    @Override
    public void close() {
        if (closed.compareAndSet(false, true)) {
            bridge.streamClose(handle);
        }
    }
}
