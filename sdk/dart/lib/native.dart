import 'dart:convert';
import 'dart:io';

import 'core_bridge.dart';
import 'registry.dart';

/// Thin HTTP transport plus the Rust-backed request preparation surface.
///
/// [prepare] asks the core for the final URL, provider auth headers (including
/// AWS SigV4), request body and wire framing, so nothing about a protocol is
/// re-implemented here. [prepareAndPost] performs one non-streaming request.
/// [stream] opens a raw incremental native stream through the core.
class NativeClient {
  final HttpClient _http;
  final CoreBridge bridge;

  NativeClient({HttpClient? httpClient, CoreBridge? coreBridge})
      : _http = httpClient ?? HttpClient(),
        bridge = coreBridge ?? CoreBridge.shared();

  /// Result of the core's `lingxi_native_prepare`: `url`, `headers`, `body`,
  /// `framing` (one of `sse`, `ndjson`, `amazon_event_stream`).
  Map<String, dynamic> prepare(
    String protocol,
    String baseUrl,
    String? apiKey,
    String? model,
    Map<String, dynamic> payload, {
    Map<String, String> headers = const {},
    int? timeoutMs,
    int? idleTimeoutMs,
    bool allowInsecureHttp = false,
    bool stream = false,
  }) {
    final request = <String, dynamic>{
      'protocol': protocol,
      'baseUrl': baseUrl,
      if (apiKey != null) 'apiKey': apiKey,
      if (model != null) 'model': model,
      'payload': payload,
      'headers': headers,
      if (timeoutMs != null) 'timeoutMs': timeoutMs,
      if (idleTimeoutMs != null) 'idleTimeoutMs': idleTimeoutMs,
      'allowInsecureHttp': allowInsecureHttp,
      'stream': stream,
    };
    return _wrapUnsupported(protocol, () => bridge.prepareNative(request));
  }

  /// Prepares through the core, then performs the non-streaming HTTP request
  /// with the provided transport and returns the raw provider JSON body.
  Future<Map<String, dynamic>> prepareAndPost(
    String protocol,
    String baseUrl,
    String? apiKey,
    String? model,
    Map<String, dynamic> payload, {
    Map<String, String> headers = const {},
    int? timeoutMs,
    bool allowInsecureHttp = false,
  }) async {
    final target = prepare(
      protocol,
      baseUrl,
      apiKey,
      model,
      payload,
      headers: headers,
      timeoutMs: timeoutMs,
      allowInsecureHttp: allowInsecureHttp,
    );
    final targetHeaders = <String, String>{};
    final rawHeaders = target['headers'];
    if (rawHeaders is Map) {
      rawHeaders.forEach((key, value) => targetHeaders['$key'] = '$value');
    }
    final body = target['body'];
    final mapBody = body is Map<String, dynamic> ? body : <String, dynamic>{};
    return postJson(
      target['url'] as String,
      targetHeaders,
      mapBody,
      allowInsecureHttp: allowInsecureHttp,
    );
  }

  /// Raw incremental stream (`unified: false`) driven by the core: events are
  /// `{event, data, id}` frames whose `data` is the untouched provider payload.
  StreamSession streamRaw(
    String protocol,
    String baseUrl,
    String? apiKey,
    String? model,
    Map<String, dynamic> payload, {
    Map<String, String> headers = const {},
    int? timeoutMs,
    int? idleTimeoutMs,
    bool allowInsecureHttp = false,
  }) {
    final request = <String, dynamic>{
      'protocol': protocol,
      'baseUrl': baseUrl,
      if (apiKey != null) 'apiKey': apiKey,
      'model': model,
      'payload': payload,
      'headers': headers,
      if (timeoutMs != null) 'timeoutMs': timeoutMs,
      if (idleTimeoutMs != null) 'idleTimeoutMs': idleTimeoutMs,
      'allowInsecureHttp': allowInsecureHttp,
    };
    return _wrapUnsupported(protocol, () => StreamSession.open(request));
  }

  /// Raw HTTP POST, kept for tests and callers that need the untouched upstream
  /// response. Redirects are refused and non-2xx responses raise.
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
    final decoded = jsonDecode(body);
    if (decoded is! Map<String, dynamic>) {
      throw HttpException('Upstream body is not a JSON object');
    }
    return decoded;
  }

  /// Auth headers as the core computes them, for callers that only need them.
  Map<String, String> buildHeaders(String protocol, String? apiKey) {
    final target = prepare(protocol, 'https://example.invalid', apiKey, 'model', const {});
    final out = <String, String>{};
    final rawHeaders = target['headers'];
    if (rawHeaders is Map) {
      rawHeaders.forEach((key, value) => out['$key'] = '$value');
    }
    return out;
  }

  void close() => _http.close(force: true);

  T _wrapUnsupported<T>(String protocol, T Function() body) {
    try {
      return body();
    } on CoreException catch (error) {
      if (error.code == 'unsupported_feature') {
        throw UnsupportedFeatureException.fromCoreMessage(protocol, error.message);
      }
      rethrow;
    }
  }
}

/// A capability the target protocol cannot express. Nothing is silently
/// downgraded: the request fails with this exception instead.
class UnsupportedFeatureException implements Exception {
  final String protocol;
  final String feature;

  UnsupportedFeatureException(this.protocol, this.feature);

  /// Parses the core message `unsupported feature 'x' for protocol 'y'`.
  factory UnsupportedFeatureException.fromCoreMessage(
      String protocol, String message) {
    const marker = "unsupported feature '";
    final start = message.indexOf(marker);
    if (start < 0) return UnsupportedFeatureException(protocol, 'unknown');
    final nameStart = start + marker.length;
    final end = message.indexOf("'", nameStart);
    return UnsupportedFeatureException(
      protocol,
      end < 0 ? 'unknown' : message.substring(nameStart, end),
    );
  }

  @override
  String toString() => "Unsupported feature '$feature' for protocol '$protocol'";
}
