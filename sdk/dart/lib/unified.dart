import 'dart:async';

import 'core_bridge.dart';
import 'native.dart';

export 'core_bridge.dart' show CoreException, StreamSession;
export 'native.dart' show NativeClient, UnsupportedFeatureException;

/// LingXi Unified client: LMP requests in, LMP responses out.
///
/// Every protocol decision lives in the Rust core: payload translation,
/// endpoint joining, provider auth (including AWS SigV4), response and stream
/// normalization. This class only moves JSON across the C ABI and performs the
/// HTTP call with the host transport for non-streaming requests; streaming is
/// pulled incrementally from a core-owned handle, so no SSE parsing happens in
/// Dart.
class UnifiedClient {
  final NativeClient nativeClient;
  final CoreBridge bridge;

  UnifiedClient({NativeClient? nativeClient, CoreBridge? coreBridge})
      : nativeClient = nativeClient ?? NativeClient(coreBridge: coreBridge),
        bridge = coreBridge ?? (nativeClient?.bridge ?? CoreBridge.shared());

  /// True when the Rust core library is loadable.
  static bool get available => CoreBridge.available;

  /// Minimal LMP text request for a single user prompt.
  static Map<String, dynamic> textRequest(String model, String prompt) => {
        'model': model,
        'messages': [
          {
            'role': 'user',
            'content': [
              {'type': 'text', 'text': prompt},
            ],
          },
        ],
      };

  /// Non-streaming completion. [request] is an LMP request map.
  Future<Map<String, dynamic>> complete(
    String protocol,
    String baseUrl,
    String? apiKey,
    Map<String, dynamic> request,
  ) async {
    final target = await _call(
      protocol,
      () => bridge.prepareUnified({
        'protocol': protocol,
        'baseUrl': baseUrl,
        if (apiKey != null) 'apiKey': apiKey,
        'request': request,
      }),
    );
    final headers = <String, String>{};
    final rawHeaders = target['headers'];
    if (rawHeaders is Map) {
      rawHeaders.forEach((key, value) => headers['$key'] = '$value');
    }
    final body = target['body'];
    final raw = await nativeClient.postJson(
      target['url'] as String,
      headers,
      body is Map<String, dynamic> ? body : <String, dynamic>{},
    );
    return _call(protocol, () => bridge.normalizeResponse(protocol, raw));
  }

  /// Convenience single-prompt completion returning the LMP response map.
  Future<Map<String, dynamic>> completeText(
    String protocol,
    String baseUrl,
    String? apiKey,
    String model,
    String prompt,
  ) =>
      complete(protocol, baseUrl, apiKey, textRequest(model, prompt));

  /// Opens an incremental Unified stream of LMP events.
  StreamSession stream(
    String protocol,
    String baseUrl,
    String? apiKey,
    Map<String, dynamic> request,
  ) {
    final open = <String, dynamic>{
      'protocol': protocol,
      'baseUrl': baseUrl,
      if (apiKey != null) 'apiKey': apiKey,
      'payload': request,
    };
    return _call(protocol, () => StreamSession.open(open, unified: true));
  }

  /// LMP stream events as a broadcast-free single-subscription stream.
  ///
  /// The core handle is pulled on demand, so events appear while the provider
  /// is still writing and backpressure is consumer-driven. Closing or pausing
  /// the Dart subscription never releases the handle underneath a blocked read:
  /// the subscription is drained/settled before [StreamSession.close] runs.
  Stream<Map<String, dynamic>> streamStream(
    String protocol,
    String baseUrl,
    String? apiKey,
    Map<String, dynamic> request,
  ) {
    StreamSession? session;
    var finished = false;
    var cancelRequested = false;
    StreamController<Map<String, dynamic>>? controller;

    Future<void> run() async {
      final out = controller!;
      try {
        session = stream(protocol, baseUrl, apiKey, request);
        while (!cancelRequested) {
          final Map<String, dynamic>? event;
          try {
            event = session!.next();
          } on CoreException catch (error) {
            // A cancelled read or an already-released handle ends the stream
            // cleanly; any other failure is reported to the consumer.
            if (error.code == 'cancelled' || error.code == 'invalid_handle') break;
            rethrow;
          }
          if (event == null) break;
          // Yield between events so a pending drain-cancel can reach the core
          // before the next blocking read starts.
          out.add(event);
          await Future<void>.delayed(Duration.zero);
        }
        finished = true;
        await out.close();
      } on Object catch (error, stack) {
        finished = true;
        if (!out.isClosed) {
          out.addError(error, stack);
          await out.close();
        }
      } finally {
        finished = true;
        session?.close();
      }
    }

    final created = StreamController<Map<String, dynamic>>(
      onListen: () {
        run();
      },
      onCancel: () {
        cancelRequested = true;
        if (!finished) session?.cancel();
      },
    );
    controller = created;
    return created.stream;
  }

  /// Buffered streaming, for callers that deliberately want the whole list.
  List<Map<String, dynamic>> streamAll(
    String protocol,
    String baseUrl,
    String? apiKey,
    Map<String, dynamic> request,
  ) {
    final session = stream(protocol, baseUrl, apiKey, request);
    try {
      return session.collect();
    } finally {
      session.close();
    }
  }

  /// Normalized embeddings: `{embeddings, usage, raw}`.
  Future<Map<String, dynamic>> embed(
    String protocol,
    String baseUrl,
    String? apiKey,
    String model,
    List<String> input,
  ) async {
    return _call(
      protocol,
      () => bridge.embed({
        'protocol': protocol,
        'baseUrl': baseUrl,
        if (apiKey != null) 'apiKey': apiKey,
        'model': model,
        'input': input,
      }),
    );
  }

  /// Normalized reranking; protocols without rerank raise
  /// [UnsupportedFeatureException].
  Future<Map<String, dynamic>> rerank(
    String protocol,
    String baseUrl,
    String? apiKey,
    String model,
    String query,
    List<String> documents, {
    int? topN,
  }) async {
    return _call(
      protocol,
      () => bridge.rerank({
        'protocol': protocol,
        'baseUrl': baseUrl,
        if (apiKey != null) 'apiKey': apiKey,
        'model': model,
        'query': query,
        'documents': documents,
        if (topN != null) 'topN': topN,
      }),
    );
  }

  /// Maps core error codes onto the SDK exception surface.
  T _call<T>(String protocol, T Function() body) {
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
