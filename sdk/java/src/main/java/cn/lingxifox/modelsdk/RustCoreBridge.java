package cn.lingxifox.modelsdk;

import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Foreign-function bridge to the Rust {@code lingxi_core} library.
 *
 * <p>All LingXi protocol knowledge lives in the Rust core; this class only moves
 * JSON across the C ABI. Requires Java 22+ (final Foreign Function &amp; Memory
 * API) and the shared library produced by {@code cargo build} in {@code core/}.
 *
 * <p>The library is located from {@code LINGXI_CORE_LIB}, the
 * {@code lingxi.core.lib} system property, or a {@code core/target/{release,debug}}
 * directory walking up from the working directory.
 */
public final class RustCoreBridge {
    private static RustCoreBridge shared;
    private static String loadFailure;

    private final Arena arena = Arena.ofShared();
    private final MethodHandle translateRequest;
    private final MethodHandle normalizeResponse;
    private final MethodHandle normalizeStreamEvents;
    private final MethodHandle stringFree;
    private final MethodHandle streamOpen;
    private final MethodHandle streamOpenError;
    private final MethodHandle streamNext;
    private final MethodHandle streamCancel;
    private final MethodHandle streamClose;
    private final MethodHandle streamLastError;
    private final MethodHandle normalizerOpen;
    private final MethodHandle normalizerOpenError;
    private final MethodHandle normalizerPush;
    private final MethodHandle normalizerFinish;
    private final MethodHandle normalizerClose;
    private final MethodHandle unifiedEmbed;
    private final MethodHandle nativePrepare;
    private final MethodHandle unifiedPrepare;
    private final MethodHandle unifiedRerank;
    private final Path libraryPath;

    private RustCoreBridge(Path libraryPath) throws Throwable {
        this.libraryPath = libraryPath;
        Linker linker = Linker.nativeLinker();
        SymbolLookup lookup = SymbolLookup.libraryLookup(libraryPath, arena);

        translateRequest = downcall(linker, lookup, "lingxi_lmp_translate_request",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        normalizeResponse = downcall(linker, lookup, "lingxi_lmp_normalize_response",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        normalizeStreamEvents = downcall(linker, lookup, "lingxi_lmp_normalize_stream_events",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        nativePrepare = downcall(linker, lookup, "lingxi_native_prepare",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        unifiedPrepare = downcall(linker, lookup, "lingxi_unified_prepare",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        unifiedEmbed = downcall(linker, lookup, "lingxi_unified_embed",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        unifiedRerank = downcall(linker, lookup, "lingxi_unified_rerank",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        stringFree = downcall(linker, lookup, "lingxi_string_free",
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));

        streamOpen = downcall(linker, lookup, "lingxi_stream_open",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        streamOpenError = downcall(linker, lookup, "lingxi_stream_open_error",
            FunctionDescriptor.of(ValueLayout.ADDRESS));
        streamNext = downcall(linker, lookup, "lingxi_stream_next",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.JAVA_LONG));
        streamCancel = downcall(linker, lookup, "lingxi_stream_cancel",
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
        streamClose = downcall(linker, lookup, "lingxi_stream_close",
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
        streamLastError = downcall(linker, lookup, "lingxi_stream_last_error",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));

        normalizerOpen = downcall(linker, lookup, "lingxi_lmp_normalizer_open",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        normalizerOpenError = downcall(linker, lookup, "lingxi_normalizer_open_error",
            FunctionDescriptor.of(ValueLayout.ADDRESS));
        normalizerPush = downcall(linker, lookup, "lingxi_lmp_normalizer_push",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        normalizerFinish = downcall(linker, lookup, "lingxi_lmp_normalizer_finish",
            FunctionDescriptor.of(ValueLayout.ADDRESS, ValueLayout.ADDRESS));
        normalizerClose = downcall(linker, lookup, "lingxi_lmp_normalizer_close",
            FunctionDescriptor.ofVoid(ValueLayout.ADDRESS));
    }

    private static MethodHandle downcall(Linker linker, SymbolLookup lookup, String name,
                                         FunctionDescriptor descriptor) {
        MemorySegment symbol = lookup.find(name).orElseThrow(
            () -> new IllegalStateException("lingxi_core is missing " + name
                + "; rebuild it with: cargo build --manifest-path core/Cargo.toml"));
        return linker.downcallHandle(symbol, descriptor);
    }

    /** Returns the shared bridge, throwing a clear error when it cannot load. */
    public static synchronized RustCoreBridge shared() {
        if (shared != null) {
            return shared;
        }
        if (loadFailure != null) {
            throw new IllegalStateException(loadFailure);
        }
        Path path = locateLibrary();
        if (path == null) {
            loadFailure = "lingxi_core shared library not found; build it with "
                + "'cargo build --manifest-path core/Cargo.toml' or set LINGXI_CORE_LIB";
            throw new IllegalStateException(loadFailure);
        }
        try {
            shared = new RustCoreBridge(path);
            return shared;
        } catch (Throwable t) {
            loadFailure = "could not load " + path + ": " + t;
            throw new IllegalStateException(loadFailure, t);
        }
    }

    /** True when the Rust core can be loaded. */
    public static synchronized boolean available() {
        try {
            shared();
            return true;
        } catch (RuntimeException e) {
            return false;
        }
    }

    public Path libraryPath() {
        return libraryPath;
    }

    private static Path locateLibrary() {
        String env = System.getenv("LINGXI_CORE_LIB");
        if (env != null && !env.isEmpty() && Files.isRegularFile(Path.of(env))) {
            return Path.of(env);
        }
        String prop = System.getProperty("lingxi.core.lib");
        if (prop != null && !prop.isEmpty() && Files.isRegularFile(Path.of(prop))) {
            return Path.of(prop);
        }
        String os = System.getProperty("os.name", "").toLowerCase();
        String name = os.contains("mac") ? "liblingxi_core.dylib"
            : os.contains("win") ? "lingxi_core.dll" : "liblingxi_core.so";
        Path dir = Path.of("").toAbsolutePath();
        while (dir != null) {
            for (String profile : new String[] {"release", "debug"}) {
                Path candidate = dir.resolve("core/target/" + profile + "/" + name);
                if (Files.isRegularFile(candidate)) {
                    return candidate;
                }
            }
            dir = dir.getParent();
        }
        return null;
    }

    // ---- JSON marshalling -------------------------------------------------

    private Map<String, Object> callJson(MethodHandle handle, String... args) {
        MemorySegment[] segments = new MemorySegment[args.length];
        try (Arena call = Arena.ofConfined()) {
            for (int i = 0; i < args.length; i++) {
                segments[i] = call.allocateFrom(args[i]);
            }
            MemorySegment ptr = (MemorySegment) handle.invokeWithArguments((Object[]) segments);
            if (ptr.address() == 0) {
                throw new LingXiCoreException("lingxi_core returned a null result");
            }
            String json;
            try {
                json = ptr.reinterpret(Long.MAX_VALUE).getString(0);
            } finally {
                free(ptr);
            }
            @SuppressWarnings("unchecked")
            Map<String, Object> parsed = (Map<String, Object>) JsonParser.parse(json);
            return parsed;
        } catch (LingXiCoreException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new LingXiCoreException("lingxi_core call failed: " + e, e);
        } catch (Throwable t) {
            throw new LingXiCoreException("lingxi_core call failed: " + t, t);
        }
    }

    private void free(MemorySegment ptr) {
        try {
            stringFree.invokeExact(ptr);
        } catch (Throwable ignored) {
            // Freeing is best effort; a leak is preferable to masking a real error.
        }
    }

    /** Wraps a JSON envelope, throwing when {@code ok} is false. */
    private static Map<String, Object> requireOk(Map<String, Object> envelope, String operation) {
        if (Boolean.TRUE.equals(envelope.get("ok"))) {
            return envelope;
        }
        Object error = envelope.get("error");
        Object code = envelope.get("code");
        throw new LingXiCoreException(operation + " failed"
            + (code == null ? "" : " [" + code + "]") + ": "
            + (error == null ? "unknown error" : error),
            code == null ? null : String.valueOf(code), null);
    }

    // ---- LMP translation --------------------------------------------------

    public Map<String, Object> translateRequest(String protocol, Map<String, Object> request) {
        Map<String, Object> out = requireOk(
            callJson(translateRequest, protocol, JsonWriter.write(request)), "translate_request");
        @SuppressWarnings("unchecked")
        Map<String, Object> payload = (Map<String, Object>) out.get("payload");
        return payload;
    }

    public Map<String, Object> normalizeResponse(String protocol, Map<String, Object> raw) {
        Map<String, Object> out = requireOk(
            callJson(normalizeResponse, protocol, JsonWriter.write(raw)), "normalize_response");
        @SuppressWarnings("unchecked")
        Map<String, Object> response = (Map<String, Object>) out.get("response");
        return response;
    }

    public List<Map<String, Object>> normalizeStreamEvents(String protocol,
                                                          List<Map<String, Object>> events) {
        Map<String, Object> out = requireOk(
            callJson(normalizeStreamEvents, protocol, JsonWriter.write(events)),
            "normalize_stream_events");
        @SuppressWarnings("unchecked")
        List<Map<String, Object>> list = (List<Map<String, Object>>) out.get("events");
        return list == null ? List.of() : list;
    }

    /** Resolved wire target for a provider-native payload: {url, headers, body, framing}. */
    public Map<String, Object> prepareNative(Map<String, Object> request) {
        return requireOk(callJson(nativePrepare, JsonWriter.write(request)), "native_prepare");
    }

    /** Same, but the request is an LMP request the core translates first. */
    public Map<String, Object> prepareUnified(Map<String, Object> request) {
        return requireOk(callJson(unifiedPrepare, JsonWriter.write(request)), "unified_prepare");
    }

    public Map<String, Object> embed(Map<String, Object> request) {
        Map<String, Object> out = requireOk(
            callJson(unifiedEmbed, JsonWriter.write(request)), "unified_embed");
        @SuppressWarnings("unchecked")
        Map<String, Object> response = (Map<String, Object>) out.get("response");
        return response;
    }

    public Map<String, Object> rerank(Map<String, Object> request) {
        Map<String, Object> out = requireOk(
            callJson(unifiedRerank, JsonWriter.write(request)), "unified_rerank");
        @SuppressWarnings("unchecked")
        Map<String, Object> response = (Map<String, Object>) out.get("response");
        return response;
    }

    // ---- Incremental streams ----------------------------------------------

    /** Opens a core-owned stream. Returns 0 on failure; see {@link #streamOpenError()}. */
    public long streamOpen(Map<String, Object> request) {
        try (Arena call = Arena.ofConfined()) {
            MemorySegment arg = call.allocateFrom(JsonWriter.write(request));
            MemorySegment ptr = (MemorySegment) streamOpen.invokeWithArguments(arg);
            return ptr.address();
        } catch (Throwable t) {
            throw new LingXiCoreException("stream_open failed: " + t, t);
        }
    }

    public String streamOpenError() {
        Map<String, Object> envelope = callJson(streamOpenError);
        Object error = envelope.get("error");
        return error == null ? "unknown stream error" : String.valueOf(error);
    }

    public Map<String, Object> streamNext(long handle, long timeoutMs) {
        try {
            MemorySegment ptr = (MemorySegment) streamNext.invokeWithArguments(
                MemorySegment.ofAddress(handle), timeoutMs);
            if (ptr.address() == 0) {
                return Map.of("ok", false, "code", "error", "error", "null result");
            }
            String json;
            try {
                json = ptr.reinterpret(Long.MAX_VALUE).getString(0);
            } finally {
                free(ptr);
            }
            @SuppressWarnings("unchecked")
            Map<String, Object> parsed = (Map<String, Object>) JsonParser.parse(json);
            return parsed;
        } catch (Throwable t) {
            throw new LingXiCoreException("stream_next failed: " + t, t);
        }
    }

    public void streamCancel(long handle) {
        try {
            streamCancel.invokeExact(MemorySegment.ofAddress(handle));
        } catch (Throwable t) {
            throw new LingXiCoreException("stream_cancel failed: " + t, t);
        }
    }

    public void streamClose(long handle) {
        try {
            streamClose.invokeExact(MemorySegment.ofAddress(handle));
        } catch (Throwable ignored) {
            // Closing must never throw: the handle may already be gone.
        }
    }

    public Map<String, Object> streamLastError(long handle) {
        Map<String, Object> out = callJsonAddr(streamLastError, handle);
        return out;
    }

    private Map<String, Object> callJsonAddr(MethodHandle handle, long addr) {
        try {
            MemorySegment ptr = (MemorySegment) handle.invokeWithArguments(MemorySegment.ofAddress(addr));
            if (ptr.address() == 0) {
                return Map.of("ok", false, "error", "null result");
            }
            String json;
            try {
                json = ptr.reinterpret(Long.MAX_VALUE).getString(0);
            } finally {
                free(ptr);
            }
            @SuppressWarnings("unchecked")
            Map<String, Object> parsed = (Map<String, Object>) JsonParser.parse(json);
            return parsed;
        } catch (Throwable t) {
            throw new LingXiCoreException("lingxi_core call failed: " + t, t);
        }
    }

    // ---- Incremental normalization sessions -------------------------------

    public long normalizerOpen(String protocol) {
        try (Arena call = Arena.ofConfined()) {
            MemorySegment ptr = (MemorySegment) normalizerOpen.invokeWithArguments(
                call.allocateFrom(protocol));
            return ptr.address();
        } catch (Throwable t) {
            throw new LingXiCoreException("normalizer_open failed: " + t, t);
        }
    }

    public String normalizerOpenError() {
        Map<String, Object> envelope = callJson(normalizerOpenError);
        Object error = envelope.get("error");
        return error == null ? "unknown normalizer error" : String.valueOf(error);
    }

    public List<Map<String, Object>> normalizerPush(long handle, Map<String, Object> frame) {
        Map<String, Object> out = requireOk(
            callJsonPair(normalizerPush, handle, JsonWriter.write(frame)), "normalizer_push");
        @SuppressWarnings("unchecked")
        List<Map<String, Object>> events = (List<Map<String, Object>>) out.get("events");
        return events == null ? List.of() : events;
    }

    public List<Map<String, Object>> normalizerFinish(long handle) {
        Map<String, Object> out = requireOk(callJsonAddr(normalizerFinish, handle), "normalizer_finish");
        @SuppressWarnings("unchecked")
        List<Map<String, Object>> events = (List<Map<String, Object>>) out.get("events");
        return events == null ? List.of() : events;
    }

    public void normalizerClose(long handle) {
        try {
            normalizerClose.invokeExact(MemorySegment.ofAddress(handle));
        } catch (Throwable ignored) {
            // Closing must never throw.
        }
    }

    private Map<String, Object> callJsonPair(MethodHandle handle, long addr, String arg) {
        try (Arena call = Arena.ofConfined()) {
            MemorySegment ptr = (MemorySegment) handle.invokeWithArguments(
                MemorySegment.ofAddress(addr), call.allocateFrom(arg));
            if (ptr.address() == 0) {
                throw new LingXiCoreException("lingxi_core returned a null result");
            }
            String json;
            try {
                json = ptr.reinterpret(Long.MAX_VALUE).getString(0);
            } finally {
                free(ptr);
            }
            @SuppressWarnings("unchecked")
            Map<String, Object> parsed = (Map<String, Object>) JsonParser.parse(json);
            return parsed;
        } catch (LingXiCoreException e) {
            throw e;
        } catch (Throwable t) {
            throw new LingXiCoreException("lingxi_core call failed: " + t, t);
        }
    }

    /** Convenience: current diagnostic snapshot for a stream handle. */
    public Map<String, Object> streamState(long handle) {
        return streamLastError(handle);
    }

    public static List<Map<String, Object>> emptyEvents() {
        return new ArrayList<>(0);
    }

    public static Map<String, Object> newRequest() {
        return new LinkedHashMap<>();
    }
}
