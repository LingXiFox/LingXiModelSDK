import 'dart:convert';
import 'dart:io';
import 'registry.dart';

class NativeClient {
  final HttpClient _http = HttpClient();

  Future<Map<String, dynamic>> postJson(
    String url,
    Map<String, String> headers,
    Map<String, dynamic> payload, {
    bool allowInsecureHttp = false,
  }) async {
    ProviderRegistry.validateEndpointUrl(url, allowInsecureHttp: allowInsecureHttp);
    final req = await _http.postUrl(Uri.parse(url));
    req.followRedirects = false;
    req.headers.contentType = ContentType.json;
    req.headers.set('Accept', 'application/json');
    headers.forEach((k, v) => req.headers.set(k, v));
    req.write(jsonEncode(payload));
    final resp = await req.close();
    final body = await resp.transform(utf8.decoder).join();
    if (resp.statusCode >= 300 && resp.statusCode <= 399) {
      throw HttpException('HTTP redirect ${resp.statusCode} blocked for security');
    }
    if (resp.statusCode < 200 || resp.statusCode > 299) {
      throw HttpException('Upstream HTTP ${resp.statusCode}: $body');
    }
    return jsonDecode(body) as Map<String, dynamic>;
  }

  Map<String, String> buildHeaders(String protocol, String? apiKey) {
    switch (protocol) {
      case 'openai_chat_completions':
      case 'openai_responses':
      case 'google_vertex_ai':
      case 'cohere':
        if (apiKey == null || apiKey.isEmpty) throw StateError('Bearer API key required');
        return {'Authorization': 'Bearer $apiKey'};
      case 'anthropic_messages':
        if (apiKey == null || apiKey.isEmpty) throw StateError('x-api-key required');
        return {'x-api-key': apiKey, 'anthropic-version': '2023-06-01'};
      case 'google_gemini':
        if (apiKey == null || apiKey.isEmpty) throw StateError('x-goog-api-key required');
        return {'x-goog-api-key': apiKey};
      case 'azure_openai':
        if (apiKey == null || apiKey.isEmpty) throw StateError('api-key required');
        return {'api-key': apiKey};
      case 'ollama':
        return {};
      case 'aws_bedrock':
        if (apiKey == null || apiKey.isEmpty) throw StateError('Bedrock credentials required');
        return {'Authorization': 'Bearer $apiKey'};
      default:
        throw UnsupportedError('Unsupported protocol: $protocol');
    }
  }

  void close() => _http.close(force: true);
}
