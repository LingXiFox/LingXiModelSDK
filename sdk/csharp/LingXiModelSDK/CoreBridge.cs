using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace LingXiModelSDK.Core;

/// <summary>Failure reported by the Rust core across the C ABI, with its stable code.</summary>
public sealed class CoreException : Exception
{
    public CoreException(string message, string? code = null, Exception? innerException = null)
        : base(message, innerException) => Code = code;

    /// <summary>Stable core error code, e.g. <c>unsupported_feature</c> or <c>auth_missing</c>.</summary>
    public string? Code { get; }
}

/// <summary>
/// Foreign-function bridge to the Rust <c>lingxi_core</c> library.
///
/// <para>All LingXi protocol knowledge (payload translation, endpoint joining,
/// provider auth, AWS SigV4, response and stream normalization) lives in the
/// Rust core; this class only moves JSON across the C ABI.</para>
///
/// <para>The library is located from <c>LINGXI_CORE_LIB</c>, then a
/// <c>core/target/{release,debug}</c> directory walking up from the assembly
/// and working directory.</para>
/// </summary>
public sealed class CoreBridge
{
    private const string LibraryName = "lingxi_core";

    private static readonly object Gate = new();
    private static readonly string[] RequiredSymbols =
    {
        "lingxi_lmp_translate_request",
        "lingxi_lmp_normalize_response",
        "lingxi_lmp_normalize_stream_events",
        "lingxi_native_prepare",
        "lingxi_unified_prepare",
        "lingxi_unified_embed",
        "lingxi_unified_rerank",
        "lingxi_stream_open",
        "lingxi_stream_open_error",
        "lingxi_stream_next",
        "lingxi_stream_cancel",
        "lingxi_stream_close",
        "lingxi_stream_last_error",
        "lingxi_string_free",
    };

    private static CoreBridge? s_shared;
    private static string? s_loadFailure;
    private static IntPtr s_libraryHandle;
    private static bool s_resolverInstalled;

    /// <summary>The shared bridge, throwing a clear error when the core cannot load.</summary>
    public static CoreBridge Shared
    {
        get
        {
            lock (Gate)
            {
                if (s_shared is not null)
                {
                    return s_shared;
                }
                if (s_loadFailure is not null)
                {
                    throw new CoreException(s_loadFailure);
                }
                try
                {
                    s_shared = new CoreBridge();
                    return s_shared;
                }
                catch (CoreException e)
                {
                    s_loadFailure = e.Message;
                    throw;
                }
                catch (Exception e)
                {
                    s_loadFailure = $"could not load {LibraryName}: {e.Message}";
                    throw new CoreException(s_loadFailure, innerException: e);
                }
            }
        }
    }

    /// <summary>True when the Rust core can be loaded.</summary>
    public static bool Available
    {
        get
        {
            try
            {
                _ = Shared;
                return true;
            }
            catch (CoreException)
            {
                return false;
            }
        }
    }

    /// <summary>Diagnostic message when the core is not loadable; null while healthy.</summary>
    public static string? LoadFailure
    {
        get
        {
            lock (Gate)
            {
                return s_loadFailure;
            }
        }
    }

    /// <summary>Absolute path of the loaded shared library.</summary>
    public string LibraryPath { get; }

    private CoreBridge()
    {
        var path = LocateLibrary()
            ?? throw new CoreException(
                "lingxi_core shared library not found; build it with 'cargo build --manifest-path core/Cargo.toml' or set LINGXI_CORE_LIB");
        try
        {
            s_libraryHandle = NativeLibrary.Load(path);
        }
        catch (Exception e)
        {
            throw new CoreException($"could not load {path}: {e.Message}", innerException: e);
        }
        LibraryPath = Path.GetFullPath(path);
        if (!s_resolverInstalled)
        {
            NativeLibrary.SetDllImportResolver(typeof(CoreBridge).Assembly, ResolveLibrary);
            s_resolverInstalled = true;
        }
        foreach (var symbol in RequiredSymbols)
        {
            if (!NativeLibrary.TryGetExport(s_libraryHandle, symbol, out _))
            {
                throw new CoreException(
                    $"lingxi_core is missing {symbol}; rebuild it with: cargo build --manifest-path core/Cargo.toml");
            }
        }
    }

    private static IntPtr ResolveLibrary(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
        => libraryName == LibraryName ? s_libraryHandle : IntPtr.Zero;

    private static string? LocateLibrary()
    {
        var env = Environment.GetEnvironmentVariable("LINGXI_CORE_LIB");
        if (!string.IsNullOrWhiteSpace(env) && File.Exists(env))
        {
            return env;
        }

        var names = OperatingSystem.IsMacOS()
            ? new[] { "liblingxi_core.dylib", "liblingxi_core.so" }
            : OperatingSystem.IsWindows()
                ? new[] { "lingxi_core.dll" }
                : new[] { "liblingxi_core.so", "liblingxi_core.dylib" };

        foreach (var start in new[] { AppContext.BaseDirectory, Directory.GetCurrentDirectory() })
        {
            var dir = new DirectoryInfo(start);
            while (dir is not null)
            {
                foreach (var profile in new[] { "release", "debug" })
                {
                    foreach (var name in names)
                    {
                        var candidate = Path.Combine(dir.FullName, "core", "target", profile, name);
                        if (File.Exists(candidate))
                        {
                            return candidate;
                        }
                    }
                }
                dir = dir.Parent;
            }
        }
        return null;
    }

    // ---- LMP translation -------------------------------------------------

    /// <summary>Translates an LMP request into the provider-native payload envelope.</summary>
    public JsonElement TranslateRequest(string protocol, string lmpRequestJson)
        => Require("translate_request", Call("translate_request", new[] { protocol, lmpRequestJson }, p => Native.lingxi_lmp_translate_request(p[0], p[1])), "payload");

    /// <summary>Normalizes a provider-native response body into an LMP response.</summary>
    public JsonElement NormalizeResponse(string protocol, string nativeResponseJson)
        => Require("normalize_response", Call("normalize_response", new[] { protocol, nativeResponseJson }, p => Native.lingxi_lmp_normalize_response(p[0], p[1])), "response");

    /// <summary>Normalizes a complete array of vendor stream frames into LMP stream events.</summary>
    public IReadOnlyList<JsonElement> NormalizeStreamEvents(string protocol, string eventsJson)
    {
        var envelope = Require("normalize_stream_events",
            Call("normalize_stream_events", new[] { protocol, eventsJson }, p => Native.lingxi_lmp_normalize_stream_events(p[0], p[1])), "events");
        if (envelope.ValueKind != JsonValueKind.Array)
        {
            return Array.Empty<JsonElement>();
        }
        return envelope.EnumerateArray().Select(e => e.Clone()).ToArray();
    }

    /// <summary>Resolved wire target for a provider-native payload.</summary>
    public JsonElement NativePrepare(string requestJson)
        => Require("native_prepare", Call("native_prepare", new[] { requestJson }, p => Native.lingxi_native_prepare(p[0])));

    /// <summary>Resolved wire target for an LMP request the core translates first.</summary>
    public JsonElement UnifiedPrepare(string requestJson)
        => Require("unified_prepare", Call("unified_prepare", new[] { requestJson }, p => Native.lingxi_unified_prepare(p[0])));

    /// <summary>Normalized embeddings; the core performs the upstream call.</summary>
    public JsonElement Embed(string requestJson)
        => Require("unified_embed", Call("unified_embed", new[] { requestJson }, p => Native.lingxi_unified_embed(p[0])), "response");

    /// <summary>Normalized reranking; the core performs the upstream call.</summary>
    public JsonElement Rerank(string requestJson)
        => Require("unified_rerank", Call("unified_rerank", new[] { requestJson }, p => Native.lingxi_unified_rerank(p[0])), "response");

    // ---- Incremental streams ---------------------------------------------

    /// <summary>Opens a core-owned stream. Returns <see cref="IntPtr.Zero"/> on failure.</summary>
    public IntPtr StreamOpen(string requestJson)
    {
        var arg = Utf8(requestJson);
        try
        {
            return Native.lingxi_stream_open(arg);
        }
        catch (Exception e)
        {
            throw new CoreException($"stream_open failed: {e.Message}", innerException: e);
        }
        finally
        {
            FreeUtf8(arg);
        }
    }

    /// <summary>Reason the most recent <see cref="StreamOpen"/> returned zero.</summary>
    public string StreamOpenError()
    {
        var envelope = ReadEnvelope(Native.lingxi_stream_open_error(), "stream_open_error");
        return envelope.TryGetProperty("error", out var error) && error.ValueKind == JsonValueKind.String
            ? error.GetString() ?? "unknown stream error"
            : "unknown stream error";
    }

    /// <summary>Waits up to <paramref name="timeoutMs"/> (0 = forever) for one event envelope.</summary>
    public JsonElement StreamNext(IntPtr handle, ulong timeoutMs)
    {
        try
        {
            return ReadEnvelope(Native.lingxi_stream_next(handle, timeoutMs), "stream_next");
        }
        catch (CoreException)
        {
            throw;
        }
        catch (Exception e)
        {
            throw new CoreException($"stream_next failed: {e.Message}", innerException: e);
        }
    }

    /// <summary>Requests cancellation; safe from any thread and on a released handle.</summary>
    public void StreamCancel(IntPtr handle)
    {
        try
        {
            Native.lingxi_stream_cancel(handle);
        }
        catch (Exception e)
        {
            throw new CoreException($"stream_cancel failed: {e.Message}", innerException: e);
        }
    }

    /// <summary>Releases the handle; idempotent and never throws.</summary>
    public void StreamClose(IntPtr handle)
    {
        try
        {
            Native.lingxi_stream_close(handle);
        }
        catch
        {
            // Closing must never throw: the handle may already be gone.
        }
    }

    /// <summary>Last error recorded for this handle, plus diagnostics.</summary>
    public JsonElement StreamLastError(IntPtr handle)
        => ReadEnvelope(Native.lingxi_stream_last_error(handle), "stream_last_error");

    // ---- JSON marshalling ------------------------------------------------

    private static JsonElement Call(string operation, string[] args, Func<IntPtr[], IntPtr> invoke)
    {
        var ptrs = new IntPtr[args.Length];
        try
        {
            for (var i = 0; i < args.Length; i++)
            {
                ptrs[i] = Utf8(args[i]);
            }
            return ReadEnvelope(invoke(ptrs), operation);
        }
        finally
        {
            foreach (var ptr in ptrs)
            {
                if (ptr != IntPtr.Zero)
                {
                    FreeUtf8(ptr);
                }
            }
        }
    }

    private static JsonElement ReadEnvelope(IntPtr result, string operation)
    {
        var json = ConsumeString(result, operation);
        try
        {
            using var document = JsonDocument.Parse(json);
            return document.RootElement.Clone();
        }
        catch (JsonException e)
        {
            throw new CoreException($"{operation} returned invalid JSON: {e.Message}", innerException: e);
        }
    }

    private static string ConsumeString(IntPtr result, string operation)
    {
        if (result == IntPtr.Zero)
        {
            throw new CoreException($"lingxi_core returned a null result for {operation}");
        }
        var text = Marshal.PtrToStringUTF8(result);
        try
        {
            return text ?? string.Empty;
        }
        finally
        {
            Native.lingxi_string_free(result);
        }
    }

    private static IntPtr Utf8(string text) => Marshal.StringToCoTaskMemUTF8(text);

    private static void FreeUtf8(IntPtr ptr) => Marshal.FreeCoTaskMem(ptr);

    private static JsonElement Require(string operation, JsonElement envelope, params string[] fields)
    {
        if (!envelope.TryGetProperty("ok", out var ok) || ok.ValueKind != JsonValueKind.True)
        {
            var code = envelope.TryGetProperty("code", out var codeElement) && codeElement.ValueKind == JsonValueKind.String
                ? codeElement.GetString()
                : null;
            var error = envelope.TryGetProperty("error", out var errorElement) && errorElement.ValueKind == JsonValueKind.String
                ? errorElement.GetString()
                : "unknown error";
            throw new CoreException(
                code is null ? $"{operation} failed: {error}" : $"{operation} failed [{code}]: {error}",
                code);
        }
        if (fields.Length == 0)
        {
            return envelope;
        }
        if (!envelope.TryGetProperty(fields[0], out var value))
        {
            throw new CoreException($"{operation} returned no '{fields[0]}'");
        }
        return value;
    }

    private static class Native
    {
        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_lmp_translate_request(IntPtr protocol, IntPtr lmpRequestJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_lmp_normalize_response(IntPtr protocol, IntPtr nativeResponseJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_lmp_normalize_stream_events(IntPtr protocol, IntPtr eventsJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_native_prepare(IntPtr requestJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_unified_prepare(IntPtr requestJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_unified_embed(IntPtr requestJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_unified_rerank(IntPtr requestJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_stream_open(IntPtr requestJson);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_stream_open_error();

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_stream_next(IntPtr handle, ulong timeoutMs);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void lingxi_stream_cancel(IntPtr handle);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void lingxi_stream_close(IntPtr handle);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr lingxi_stream_last_error(IntPtr handle);

        [DllImport(LibraryName, ExactSpelling = true, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void lingxi_string_free(IntPtr ptr);
    }
}

/// <summary>Terse readers over the core's camelCase JSON envelopes.</summary>
internal static class JsonRead
{
    public static string? OptString(this JsonElement element, string name)
        => element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(name, out var value)
            && value.ValueKind == JsonValueKind.String
                ? value.GetString()
                : null;

    public static long? OptLong(this JsonElement element, string name)
    {
        if (element.ValueKind != JsonValueKind.Object || !element.TryGetProperty(name, out var value))
        {
            return null;
        }
        if (value.ValueKind != JsonValueKind.Number)
        {
            return null;
        }
        if (value.TryGetInt64(out var integer))
        {
            return integer;
        }
        var number = value.GetDouble();
        return Math.Round(number) == number ? (long)number : null;
    }

    public static double? OptDouble(this JsonElement element, string name)
        => element.ValueKind == JsonValueKind.Object
            && element.TryGetProperty(name, out var value)
            && value.ValueKind == JsonValueKind.Number
                ? value.GetDouble()
                : null;

    public static bool? OptBool(this JsonElement element, string name)
    {
        if (element.ValueKind != JsonValueKind.Object || !element.TryGetProperty(name, out var value))
        {
            return null;
        }
        return value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => null,
        };
    }
}
