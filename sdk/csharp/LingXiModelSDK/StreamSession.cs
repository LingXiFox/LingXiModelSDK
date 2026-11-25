using System.Text.Json;

namespace LingXiModelSDK.Core;

/// <summary>
/// Incremental pull session over <c>lingxi_stream_open</c>/<c>lingxi_stream_next</c>.
///
/// Every <see cref="Next"/> call blocks the calling thread; the core delivers one
/// frame per pull, which is how backpressure reaches the upstream connection.
/// </summary>
public sealed class StreamSession : IDisposable
{
    public const ulong DefaultTimeoutMs = 60_000;

    private readonly CoreBridge _bridge;
    private IntPtr _handle;
    private int _closed;

    private StreamSession(CoreBridge bridge, IntPtr handle)
    {
        _bridge = bridge;
        _handle = handle;
    }

    /// <summary>Opens a stream for the given camelCase request envelope.</summary>
    public static StreamSession Open(CoreBridge bridge, string requestJson)
    {
        var handle = bridge.StreamOpen(requestJson);
        if (handle == IntPtr.Zero)
        {
            throw new CoreException("stream_open failed: " + bridge.StreamOpenError());
        }
        return new StreamSession(bridge, handle);
    }

    public bool Closed => Volatile.Read(ref _closed) != 0;

    /// <summary>
    /// Pulls the next frame; null means the stream ended cleanly.
    /// </summary>
    public JsonElement? Next(ulong timeoutMs = DefaultTimeoutMs)
    {
        if (Closed)
        {
            throw new InvalidOperationException("stream session is closed");
        }

        var envelope = _bridge.StreamNext(_handle, timeoutMs);
        if (!(envelope.TryGetProperty("ok", out var ok) && ok.ValueKind == JsonValueKind.True))
        {
            var code = envelope.OptString("code");
            var error = envelope.OptString("error") ?? "unknown error";
            throw new CoreException(
                code is null ? "stream failed: " + error : "stream failed [" + code + "]: " + error,
                code);
        }

        if (envelope.TryGetProperty("done", out var done) && done.ValueKind == JsonValueKind.True)
        {
            return null;
        }

        if (envelope.TryGetProperty("event", out var @event))
        {
            return @event.Clone();
        }

        throw new CoreException("stream_next returned no event");
    }

    /// <summary>Requests cancellation from any thread; the pending pull fails with a cancel code.</summary>
    public void Cancel()
    {
        if (Closed)
        {
            return;
        }
        _bridge.StreamCancel(_handle);
    }

    /// <summary>Diagnostics recorded by the core for this handle.</summary>
    public JsonElement LastError => _bridge.StreamLastError(_handle);

    /// <summary>Releases the core handle; idempotent.</summary>
    public void Close()
    {
        if (Interlocked.Exchange(ref _closed, 1) != 0)
        {
            return;
        }
        _bridge.StreamClose(_handle);
        _handle = IntPtr.Zero;
    }

    public void Dispose() => Close();
}
