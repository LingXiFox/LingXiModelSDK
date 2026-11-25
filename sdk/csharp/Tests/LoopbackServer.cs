using System.Net;
using System.Net.Sockets;
using System.Text;

namespace LingXiModelSdkTests;

/// <summary>
/// Loopback HTTP server replaying the conformance fixtures.
///
/// Besides plain fixture replay it models two streaming behaviours:
/// an incremental stream that is held open until the client acknowledges the
/// first frame, and a stream that stalls after one frame so cancellation can be
/// exercised.
/// </summary>
internal sealed class LoopbackServer : IDisposable
{
    private const string OpenAiStreamFirst = "data: {\"id\":\"chatcmpl-stream-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hello \"}}]}\n\n";
    private const string OpenAiStreamSecond = "data: {\"id\":\"chatcmpl-stream-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"world\"}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":2,\"total_tokens\":12}}\n\n";
    private const string DoneFrame = "data: [DONE]\n\n";

    private readonly HttpListener _listener;
    private readonly CancellationTokenSource _shutdown = new();
    private readonly Task _loop;
    private readonly string _root;

    private int _catalogHits;
    private int _catalogMode;
    private int _incrementalHits;

    public LoopbackServer(string root)
    {
        _root = root;
        Port = GetFreePort();
        _listener = new HttpListener();
        _listener.Prefixes.Add($"http://127.0.0.1:{Port}/");
        _listener.Start();
        _loop = Task.Run(AcceptAsync);
    }

    public int Port { get; }

    public string BaseUrl => $"http://127.0.0.1:{Port}";

    /// <summary>Number of <c>models.json</c> requests that reached the server.</summary>
    public int CatalogHits => Volatile.Read(ref _catalogHits);

    /// <summary>Zero serves the catalog, one answers 503.</summary>
    public int CatalogMode
    {
        get => Volatile.Read(ref _catalogMode);
        set => Volatile.Write(ref _catalogMode, value);
    }

    public int IncrementalHits => Volatile.Read(ref _incrementalHits);

    /// <summary>Completed by the test once it has consumed the first streamed frame.</summary>
    public TaskCompletionSource FirstFrameConsumed { get; } =
        new(TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>Set once the server stalled a stream after its first frame.</summary>
    public TaskCompletionSource StalledAfterFirstFrame { get; } =
        new(TaskCreationOptions.RunContinuationsAsynchronously);

    /// <summary>Authorization header seen on the last Bedrock Converse request.</summary>
    public string? LastBedrockAuthorization { get; private set; }

    private static int GetFreePort()
    {
        using var listener = new TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        return ((IPEndPoint)listener.LocalEndpoint).Port;
    }

    private async Task AcceptAsync()
    {
        while (!_shutdown.IsCancellationRequested)
        {
            HttpListenerContext context;
            try
            {
                context = await _listener.GetContextAsync().ConfigureAwait(false);
            }
            catch
            {
                return;
            }
            _ = Task.Run(() => DispatchAsync(context));
        }
    }

    private async Task DispatchAsync(HttpListenerContext context)
    {
        try
        {
            await HandleAsync(context).ConfigureAwait(false);
        }
        catch (Exception)
        {
            // A cancelled stream tears the connection down mid-write; expected.
        }
        finally
        {
            try
            {
                context.Response.Close();
            }
            catch
            {
            }
        }
    }

    private async Task HandleAsync(HttpListenerContext context)
    {
        var path = context.Request.Url?.AbsolutePath ?? "/";
        var authorization = context.Request.Headers["Authorization"];

        if (path.StartsWith("/redir", StringComparison.Ordinal))
        {
            context.Response.StatusCode = 302;
            context.Response.RedirectLocation = "http://127.0.0.1:9/leak";
            return;
        }

        if (path.StartsWith("/models.json", StringComparison.Ordinal))
        {
            await HandleCatalogAsync(context).ConfigureAwait(false);
            return;
        }

        if (path.StartsWith("/converse", StringComparison.Ordinal) || path.Contains("/converse", StringComparison.Ordinal))
        {
            LastBedrockAuthorization = authorization;
            await WriteFixtureAsync(context, "aws_bedrock_response.json", "application/json").ConfigureAwait(false);
            return;
        }

        if (path.Contains("/v2/rerank", StringComparison.Ordinal))
        {
            await WriteFixtureAsync(context, "cohere_rerank_response.json", "application/json").ConfigureAwait(false);
            return;
        }

        if (path.EndsWith("/embeddings", StringComparison.Ordinal))
        {
            await WriteFixtureAsync(context, "openai_embed_response.json", "application/json").ConfigureAwait(false);
            return;
        }

        if (path.Contains(":generateContent", StringComparison.Ordinal))
        {
            await WriteFixtureAsync(context, "google_gemini_response.json", "application/json").ConfigureAwait(false);
            return;
        }

        if (path.StartsWith("/cancel/", StringComparison.Ordinal))
        {
            await WriteStalledStreamAsync(context).ConfigureAwait(false);
            return;
        }

        if (path.StartsWith("/incremental/", StringComparison.Ordinal))
        {
            await WriteIncrementalStreamAsync(context).ConfigureAwait(false);
            return;
        }

        if (path.Contains("/messages", StringComparison.Ordinal))
        {
            if (await RequestIsStreamingAsync(context).ConfigureAwait(false))
            {
                await WriteFixtureAsync(context, "anthropic_messages_stream.sse", "text/event-stream").ConfigureAwait(false);
            }
            else
            {
                await WriteFixtureAsync(context, "anthropic_messages_response.json", "application/json").ConfigureAwait(false);
            }
            return;
        }

        if (path.Contains("/chat/completions", StringComparison.Ordinal))
        {
            if (await RequestIsStreamingAsync(context).ConfigureAwait(false))
            {
                await WriteFixtureAsync(context, "openai_chat_stream.sse", "text/event-stream").ConfigureAwait(false);
            }
            else
            {
                await WriteFixtureAsync(context, "openai_chat_response.json", "application/json").ConfigureAwait(false);
            }
            return;
        }

        await WriteFixtureAsync(context, "cohere_chat_response.json", "application/json").ConfigureAwait(false);
    }

    private async Task HandleCatalogAsync(HttpListenerContext context)
    {
        Interlocked.Increment(ref _catalogHits);
        if (CatalogMode == 1)
        {
            context.Response.StatusCode = 503;
            return;
        }

        var etag = "\"rev-1\"";
        if (context.Request.Headers["If-None-Match"] == etag)
        {
            context.Response.StatusCode = 304;
            context.Response.AddHeader("ETag", etag);
            return;
        }

        var body = await File.ReadAllBytesAsync(Path.Combine(_root, "conformance", "catalog", "fixtures", "v2_catalog.json"))
            .ConfigureAwait(false);
        context.Response.StatusCode = 200;
        context.Response.ContentType = "application/json";
        context.Response.AddHeader("ETag", etag);
        context.Response.ContentLength64 = body.Length;
        await context.Response.OutputStream.WriteAsync(body).ConfigureAwait(false);
    }

    private async Task WriteFixtureAsync(HttpListenerContext context, string name, string contentType)
    {
        var bytes = await File.ReadAllBytesAsync(Path.Combine(_root, "conformance", "native", name)).ConfigureAwait(false);
        context.Response.StatusCode = 200;
        context.Response.ContentType = contentType;
        context.Response.ContentLength64 = bytes.Length;
        await context.Response.OutputStream.WriteAsync(bytes).ConfigureAwait(false);
    }

    /// <summary>
    /// Streams the first frame immediately and only continues once the client has
    /// seen it, proving events are delivered while the server is still writing.
    /// </summary>
    private async Task WriteIncrementalStreamAsync(HttpListenerContext context)
    {
        Interlocked.Increment(ref _incrementalHits);
        context.Response.StatusCode = 200;
        context.Response.ContentType = "text/event-stream";
        context.Response.SendChunked = true;
        var output = context.Response.OutputStream;

        await WriteAsync(output, OpenAiStreamFirst).ConfigureAwait(false);
        await FirstFrameConsumed.Task.WaitAsync(TimeSpan.FromSeconds(20), _shutdown.Token).ConfigureAwait(false);
        await WriteAsync(output, OpenAiStreamSecond).ConfigureAwait(false);
        await WriteAsync(output, DoneFrame).ConfigureAwait(false);
    }

    /// <summary>Writes one frame then stalls, so the client must cancel mid-read.</summary>
    private async Task WriteStalledStreamAsync(HttpListenerContext context)
    {
        context.Response.StatusCode = 200;
        context.Response.ContentType = "text/event-stream";
        context.Response.SendChunked = true;
        await WriteAsync(context.Response.OutputStream, OpenAiStreamFirst).ConfigureAwait(false);
        StalledAfterFirstFrame.TrySetResult();
        await Task.Delay(TimeSpan.FromSeconds(30), _shutdown.Token).ConfigureAwait(false);
    }

    private static async Task WriteAsync(Stream output, string frame)
    {
        var bytes = Encoding.UTF8.GetBytes(frame);
        await output.WriteAsync(bytes).ConfigureAwait(false);
        await output.FlushAsync().ConfigureAwait(false);
    }

    private static async Task<bool> RequestIsStreamingAsync(HttpListenerContext context)
    {
        using var reader = new StreamReader(context.Request.InputStream, Encoding.UTF8);
        var body = await reader.ReadToEndAsync().ConfigureAwait(false);
        return body.Contains("\"stream\":true", StringComparison.Ordinal);
    }

    public void Dispose()
    {
        _shutdown.Cancel();
        try
        {
            _listener.Stop();
            _listener.Close();
        }
        catch
        {
        }
        try
        {
            _loop.Wait(TimeSpan.FromSeconds(2));
        }
        catch
        {
        }
        _shutdown.Dispose();
    }
}
