using System.Net;
using System.Net.Sockets;
using LingXiModelSDK.Catalog;
using LingXiModelSDK.Registry;
using LingXiModelSDK.Unified;

static string FindRepoRoot()
{
    foreach (var start in new[] { Directory.GetCurrentDirectory(), AppContext.BaseDirectory })
    {
        var dir = new DirectoryInfo(start);
        while (dir != null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "registry", "registry.json")))
                return dir.FullName;
            dir = dir.Parent;
        }
    }
    throw new DirectoryNotFoundException("Could not locate repository root containing registry/registry.json");
}

static int GetFreePort()
{
    using var l = new TcpListener(IPAddress.Loopback, 0);
    l.Start();
    return ((IPEndPoint)l.LocalEndpoint).Port;
}

var root = FindRepoRoot();
var regJson = await File.ReadAllTextAsync(Path.Combine(root, "registry", "registry.json"));
var reg = ProviderRegistry.FromJson(regJson);
if (reg.Count < 320) throw new Exception($"Expected >=320 providers, got {reg.Count}");
var ep = reg.ResolveEndpoint("amazon-bedrock", "aws_bedrock");
if (!ep.Contains("bedrock-runtime")) throw new Exception($"Unexpected endpoint: {ep}");

var cat = ModelCatalog.Decode("{\"schemaVersion\":\"2.0\",\"providers\":{\"openai\":{\"id\":\"openai\"}}}");
if (!cat.Providers.ContainsKey("openai")) throw new Exception("Catalog decode failed");

using var listener = new HttpListener();
var port = GetFreePort();
listener.Prefixes.Add($"http://127.0.0.1:{port}/");
listener.Start();
var srvTask = Task.Run(async () =>
{
    while (listener.IsListening)
    {
        HttpListenerContext ctx;
        try { ctx = await listener.GetContextAsync(); }
        catch { break; }
        var path = ctx.Request.Url?.AbsolutePath ?? "";
        if (path.StartsWith("/redir"))
        {
            ctx.Response.StatusCode = 302;
            ctx.Response.RedirectLocation = "http://127.0.0.1:9/leak";
            ctx.Response.Close();
            continue;
        }
        var fixture = path.Contains("converse")
            ? "aws_bedrock_response.json"
            : path.Contains("rerank") ? "cohere_rerank_response.json" : "cohere_chat_response.json";
        if (path.Contains("converse") && !(ctx.Request.Headers["Authorization"] ?? "").StartsWith("AWS4-HMAC-SHA256"))
            throw new Exception("Missing SigV4 header");
        var bytes = await File.ReadAllBytesAsync(Path.Combine(root, "conformance", "native", fixture));
        ctx.Response.ContentType = "application/json";
        ctx.Response.ContentLength64 = bytes.Length;
        await ctx.Response.OutputStream.WriteAsync(bytes);
        ctx.Response.Close();
    }
});

try
{
    var baseUrl = $"http://127.0.0.1:{port}";
    var client = new UnifiedClient();
    using var bDoc = await client.CompleteTextAsync("aws_bedrock", baseUrl, "AKIA:SECRET", "anthropic.claude-3-5-sonnet", "hi");
    if (!bDoc.RootElement.TryGetProperty("output", out _)) throw new Exception("Missing bedrock output");

    using var rrDoc = await client.RerankAsync("cohere", baseUrl, "co-key", "rerank-v3.5", "cs", new[] { "a", "b" });
    if (!rrDoc.RootElement.TryGetProperty("results", out _)) throw new Exception("Missing rerank results");

    var threwUnsupported = false;
    try { await client.RerankAsync("openai_chat_completions", baseUrl, "sk", "m", "q", new[] { "a" }); }
    catch (UnsupportedFeatureException) { threwUnsupported = true; }
    if (!threwUnsupported) throw new Exception("Expected UnsupportedFeatureException");

    var threwRedirect = false;
    try { await client.CompleteTextAsync("cohere", baseUrl + "/redir", "co-key", "command-a", "hi"); }
    catch (InvalidOperationException) { threwRedirect = true; }
    if (!threwRedirect) throw new Exception("Expected redirect block");

    Console.WriteLine("C# 4-module SDK tests passed!");
}
finally
{
    listener.Stop();
    await srvTask;
}
