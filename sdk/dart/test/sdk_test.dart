import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:isolate';

import '../lib/catalog.dart';
import '../lib/catalog_loader.dart';
import '../lib/core_bridge.dart';
import '../lib/native.dart';
import '../lib/registry.dart';
import '../lib/unified.dart' show UnifiedClient;

/// End-to-end checks for the Dart SDK surface: Catalog (+ loader), Registry,
/// Native and Unified, all backed by the shared Rust core.
///
/// Provider checks run against a loopback HTTP server that replays the
/// repository conformance fixtures, so no credentials or network access are
/// required. The server lives in a second isolate: a synchronous
/// lingxi_stream_next FFI call blocks the calling isolate, and with the server
/// in that same isolate nothing could be flushed until the stream had ended.
class Expect {
  int count = 0;

  void call(bool condition, String message) {
    if (!condition) throw AssertionError(message);
    count++;
  }
}

Future<void> main() async {
  final expect = Expect();
  final root = Directory('../..').absolute.path + '/';

  final registry = ProviderRegistry.fromJson(
      await File(root + 'registry/registry.json').readAsString());
  expect(registry.length >= 320,
      'Expected >= 320 providers, got ' + registry.length.toString());
  final bedrockEndpoint = registry.resolveEndpoint('amazon-bedrock', 'aws_bedrock');
  expect(bedrockEndpoint.contains('bedrock-runtime'),
      'Unexpected bedrock endpoint: ' + bedrockEndpoint);

  expect(CoreBridge.available,
      'Rust core is not loadable; build it with: cargo build --manifest-path core/Cargo.toml');
  final libraryPath = CoreBridge.loadedLibraryPath;
  expect(libraryPath != null && libraryPath.endsWith(CoreBridge.platformLibraryName),
      'Unexpected core library path: ' + libraryPath.toString());

  catalogChecks(root, expect);

  final server = await FixtureServer.start(root);
  final unified = UnifiedClient();
  final native = NativeClient();
  try {
    await unifiedChecks(unified, server, expect);
    await embedAndRerankChecks(unified, server.baseUrl, expect);
    await nativeChecks(native, server, expect);
    await streamChecks(server.baseUrl, expect);
    await securityChecks(unified, server.baseUrl, expect);
  } finally {
    native.close();
    unified.nativeClient.close();
    server.stop();
  }

  await catalogLoaderChecks(root, expect);
  print('Dart SDK tests passed (' + expect.count.toString() + ' checks)');
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

void catalogChecks(String root, Expect expect) {
  final conformance = root + 'conformance/catalog/';
  final expectations = jsonDecode(File(conformance + 'expectations.json').readAsStringSync())
      as Map<String, dynamic>;
  final catalog = LingXiModelCatalog.decode(
      File(conformance + 'fixtures/v2_catalog.json').readAsStringSync());

  expect(catalog.revision.schemaVersion == expectations['schemaVersion'], 'schemaVersion');
  expect(catalog.revision.catalogRevision == expectations['catalogRevision'], 'catalogRevision');
  expect(catalog.revision.totalProviders == expectations['totalProviders'], 'totalProviders');
  expect(expectations['totalModels'] == catalog.totalModels, 'totalModels');

  List<String> providerIds(Iterable<CatalogProvider> providers) =>
      [for (final provider in providers) provider.id];
  List<String> qualifiedIds(Iterable<CatalogModel> models) =>
      [for (final model in models) model.qualifiedId];

  expect(_same(providerIds(catalog.sortedProviders()), expectations['providerOrder']),
      'providerOrder was ' + providerIds(catalog.sortedProviders()).join(','));
  final featured = expectations['featuredProviderOrder'] as Map<String, dynamic>;
  expect(
      _same(providerIds(catalog.sortedProviders(_strings(featured['featured']))),
          featured['expected']),
      'featuredProviderOrder');
  expect(
      _same([for (final model in catalog.modelsForProvider('prov10')) model.id],
          expectations['prov10ModelOrder']),
      'prov10ModelOrder');
  expect(
      _same([for (final model in catalog.modelsForProvider('deepseek')) model.id],
          expectations['deepseekModelOrder']),
      'deepseekModelOrder');
  expect(_same(qualifiedIds(catalog.allModels()), expectations['allModelQualifiedOrder']),
      'allModelQualifiedOrder');

  final gpt10 = catalog.model('PROV10', 'GPT-10');
  expect(gpt10 != null, 'gpt-10 must resolve case-insensitively');
  expect(gpt10!.pricing.input == 0.0, 'gpt10 zero input price');
  expect(!gpt10.pricing.isUnknown, 'gpt10 pricing known');
  expect(gpt10.capabilities.supportsTemperature == false, 'gpt10 temperature=false');
  expect(gpt10.capabilities.vision, 'gpt10 vision');
  expect(gpt10.capabilities.audioInput, 'gpt10 audioInput');
  expect(gpt10.capabilities.audioOutput, 'gpt10 audioOutput');
  expect(_same(gpt10.capabilities.reasoningEfforts, ['low', 'medium', 'high']),
      'gpt10 reasoning efforts');
  final experimental = gpt10.fields['experimental'] as Map<String, dynamic>;
  expect(experimental['warmup'] == true, 'unknown experimental.warmup preserved');

  final gpt5 = catalog.model('prov10/gpt-5');
  expect(gpt5 != null, 'qualified model lookup');
  expect(gpt5!.contextWindow == 0, 'gpt5 context=0');
  expect(gpt5.pricing.isUnknown, 'gpt5 unknown pricing');
  expect(gpt5.capabilities.supportsTemperature == null, 'gpt5 nil temperature');
  expect(!gpt5.capabilities.structuredOutput, 'gpt5 structured_output=false');

  final search = expectations['search'] as Map<String, dynamic>;
  expect(_same(qualifiedIds(catalog.search('gpt', 1)), search['gptLimit1']), 'search gpt limit1');
  expect(_same(qualifiedIds(catalog.search('deepseek')), search['deepseek']), 'search deepseek');

  final filters = expectations['filters'] as Map<String, dynamic>;
  expect(
      _same(qualifiedIds(catalog.modelsMatching(const ModelFilter(vision: true))),
          filters['vision']),
      'filter vision');
  expect(
      _same(
          qualifiedIds(
              catalog.modelsMatching(const ModelFilter(minimumContextWindow: 200000))),
          filters['minContext200k']),
      'filter minimumContextWindow');
  expect(
      _same(qualifiedIds(catalog.modelsMatching(const ModelFilter(providerId: 'deepseek'))),
          filters['deepseekDefault']),
      'filter providerDefault');
  expect(
      _same(
          qualifiedIds(catalog.modelsMatching(
              const ModelFilter(providerId: 'deepseek', includeDeprecated: true))),
          filters['deepseekIncludeDeprecated']),
      'filter includeDeprecated');

  final legacy = LingXiModelCatalog.decode(
      File(conformance + 'fixtures/v1_legacy.json').readAsStringSync());
  expect(legacy.revision.schemaVersion == '1.0', 'legacy schemaVersion');
  expect(legacy.model('ollama', 'llama3')!.contextWindow == 8192, 'legacy context');

  final malformed = LingXiModelCatalog.decode(
      File(conformance + 'fixtures/malformed_entry.json').readAsStringSync());
  expect(malformed.model('p', 'published-key') != null, 'published-key present');
  expect(malformed.model('p', 'internal-id') == null, 'internal-id absent');

  expect(
      _catalogError(
          () => LingXiModelCatalog.decode(
              File(conformance + 'fixtures/unsupported_schema.json').readAsStringSync()),
          CatalogErrorKind.unsupportedSchemaVersion),
      'unsupported schema kind');
  expect(
      _catalogError(
          () => LingXiModelCatalog.decode(
              File(conformance + 'fixtures/broken_empty_providers.json').readAsStringSync()),
          CatalogErrorKind.invalidCatalog),
      'empty providers kind');
}

// ---------------------------------------------------------------------------
// Unified
// ---------------------------------------------------------------------------

Future<void> unifiedChecks(UnifiedClient client, FixtureServer server, Expect expect) async {
  final base = server.baseUrl;
  final bedrock = await client.completeText(
      'aws_bedrock', base, 'AKID:SECRET', 'anthropic.claude-3-5-sonnet', 'hi');
  expect(_textOf(bedrock) == 'Hello from AWS Bedrock',
      'bedrock text was ' + _textOf(bedrock));
  expect(bedrock['finishReason'] == 'tool_use',
      'bedrock finishReason was ' + bedrock['finishReason'].toString());
  final usage = bedrock['usage'] as Map<String, dynamic>;
  expect(usage['inputTokens'] == 24, 'bedrock input tokens');
  expect(usage['cacheReadTokens'] == 8, 'bedrock cache read tokens');
  final thinking = _blocksOf(bedrock, 'thinking');
  expect(thinking.length == 1 && thinking.first['signature'] == 'sig-bedrock-1',
      'bedrock thinking signature');
  final toolCalls = _blocksOf(bedrock, 'tool_call');
  expect(toolCalls.length == 1 && toolCalls.first['name'] == 'get_weather',
      'bedrock tool call');
  expect(!bedrock.containsKey('output'), 'unified response must not leak raw provider fields');
  expect(bedrock.containsKey('content'), 'LMP response must carry content blocks');

  final cohere = await client.completeText('cohere', base, 'co-key', 'command-a', 'hi');
  expect(_textOf(cohere) == 'Hello from Cohere', 'cohere text was ' + _textOf(cohere));
  expect(cohere['finishReason'] == 'COMPLETE', 'cohere finishReason');

  final chat = await client.completeText(
      'openai_chat_completions', base, 'sk-test', 'gpt-4o-mini', 'hi');
  expect(_textOf(chat) == 'Hello from chat completions',
      'openai text was ' + _textOf(chat));
  expect(chat['finishReason'] == 'stop', 'openai finishReason');

  final gemini =
      await client.completeText('google_gemini', base, 'g-key', 'gemini-2.0-flash', 'hi');
  expect(_textOf(gemini) == 'Hello from Gemini', 'gemini text was ' + _textOf(gemini));
  expect(gemini['finishReason'] == 'STOP', 'gemini finishReason');
  expect(_blocksOf(gemini, 'thinking').length == 1, 'gemini thinking blocks');
  expect((gemini['usage'] as Map<String, dynamic>)['totalTokens'] == 23,
      'gemini total tokens');

  final converse = await server.waitForRequest((request) =>
      (request['path'] as String).contains('converse') && request['auth'] != null);
  expect(converse != null, 'no converse request reached the fixture server');
  final authorization = converse!['auth'] as String;
  expect(authorization.startsWith('AWS4-HMAC-SHA256'),
      'unified bedrock must carry SigV4, got ' + authorization);
  expect(!authorization.contains('SECRET'), 'AWS secret must never leak into headers');
}

Future<void> embedAndRerankChecks(UnifiedClient client, String base, Expect expect) async {
  final embed = await client.embed(
      'openai_chat_completions', base, 'sk-test', 'text-embedding-3-small', ['a', 'b']);
  final vectors = embed['embeddings'] as List;
  expect(vectors.length == 2, 'embedding count was ' + vectors.length.toString());
  expect((vectors.first as List).length == 3, 'embedding dimensions');
  expect(embed.containsKey('usage'), 'embedding response must carry usage');

  final rerank =
      await client.rerank('cohere', base, 'co-key', 'rerank-v3.5', 'dart', ['a', 'b']);
  final results = rerank['results'] as List;
  expect(results.length == 2, 'rerank result count');
  expect((results.first as Map)['index'] == 1, 'rerank top index');
  expect(((results.first as Map)['relevanceScore'] as num).toDouble() == 0.94,
      'rerank top score');

  var rerankUnsupported = '';
  try {
    await client.rerank('openai_chat_completions', base, 'sk', 'm', 'q', ['a']);
  } on UnsupportedFeatureException catch (error) {
    rerankUnsupported = error.protocol + '/' + error.feature;
  }
  expect(rerankUnsupported == 'openai_chat_completions/rerank',
      'rerank unsupported case was ' + rerankUnsupported);

  var embedUnsupported = '';
  try {
    await client.embed('anthropic_messages', base, 'sk-ant', 'm', ['a']);
  } on UnsupportedFeatureException catch (error) {
    embedUnsupported = error.protocol + '/' + error.feature;
  }
  expect(embedUnsupported == 'anthropic_messages/embed',
      'embed unsupported case was ' + embedUnsupported);
}

// ---------------------------------------------------------------------------
// Native
// ---------------------------------------------------------------------------

Future<void> nativeChecks(NativeClient client, FixtureServer server, Expect expect) async {
  final payload = <String, dynamic>{
    'messages': [
      {
        'role': 'user',
        'content': [
          {'text': 'hi'},
        ],
      },
    ],
  };
  final target = client.prepare('aws_bedrock', server.baseUrl, 'AKID:SECRET',
      'anthropic.claude-3-5-sonnet', payload);
  final url = target['url'] as String;
  expect(url.endsWith('/model/anthropic.claude-3-5-sonnet/converse'),
      'native bedrock url was ' + url);
  expect(target['framing'] == 'amazon_event_stream',
      'unexpected native framing ' + target['framing'].toString());
  final headers = (target['headers'] as Map).cast<String, dynamic>();
  final authorization = headers['Authorization'] as String;
  expect(authorization.startsWith('AWS4-HMAC-SHA256'),
      'sigv4 header was ' + authorization);
  expect(!authorization.contains('SECRET'), 'secret must not leak into headers');
  expect(_headerValue(headers, 'x-amz-date') != null, 'sigv4 must send x-amz-date');
  expect((target['body'] as Map)['messages'] is List,
      'native prepare must echo the provider body it will send');

  final raw = await client.prepareAndPost('aws_bedrock', server.baseUrl, 'AKID:SECRET',
      'anthropic.claude-3-5-sonnet', payload);
  expect(raw['output'].toString().contains('Hello from AWS Bedrock'), 'native raw body');

  final openaiTarget = client.prepare('openai_chat_completions', server.baseUrl, 'sk-test',
      'gpt-4o-mini', {
    'model': 'gpt-4o-mini',
    'messages': [
      {'role': 'user', 'content': 'hi'},
    ],
  });
  expect(openaiTarget['url'].toString().endsWith('/chat/completions'),
      'native openai url ' + openaiTarget['url'].toString());
  expect(openaiTarget['framing'] == 'sse', 'native openai framing');
  expect(((openaiTarget['headers'] as Map)['Authorization']) == 'Bearer sk-test',
      'native openai bearer header');

  final signed = await server.waitForRequest((request) =>
      (request['path'] as String).contains('converse') &&
      (request['auth'] as String).startsWith('AWS4-HMAC-SHA256'));
  expect(signed != null, 'raw native post must also be SigV4 signed');
  expect(
      await server.waitForRequest(
              (request) => (request['path'] as String).endsWith('/chat/completions')) !=
          null,
      'native openai request must reach the fixture server');
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

Future<void> streamChecks(String base, Expect expect) async {
  final unifiedRequest = <String, dynamic>{
    'model': 'claude',
    'messages': [
      {
        'role': 'user',
        'content': [
          {'type': 'text', 'text': 'hi'},
        ],
      },
    ],
  };
  final client = UnifiedClient();
  final clock = Stopwatch()..start();
  final arrivals = <int>[];
  final text = StringBuffer();
  final thinking = StringBuffer();
  final types = <String>{};
  var sawEnd = false;
  String? endReason;

  await for (final event
      in client.streamStream('anthropic_messages', base, 'sk-ant', unifiedRequest)) {
    arrivals.add(clock.elapsedMilliseconds);
    final type = event['type'] as String?;
    if (type != null) types.add(type);
    if (type == 'text_delta') text.write(event['delta']);
    if (type == 'thinking_delta') thinking.write(event['delta']);
    if (type == 'message_end') {
      sawEnd = true;
      endReason = event['finishReason'] as String?;
    }
  }
  expect(types.isNotEmpty, 'unified stream produced no typed events');
  expect(text.toString() == 'Hello Anthropic', 'streamed text was ' + text.toString());
  expect(thinking.toString() == 'Thinking...',
      'streamed thinking was ' + thinking.toString());
  expect(sawEnd, 'LMP stream must end with message_end');
  expect(endReason == null || endReason == 'stop' || endReason == 'end_turn',
      'unexpected stream finish reason: ' + endReason.toString());
  expect(arrivals.length >= 3,
      'expected at least 3 LMP events, got ' + arrivals.length.toString());
  expect(arrivals.first < 400,
      'first LMP event arrived only at ' + arrivals.first.toString() + ' ms');
  expect(arrivals.last - arrivals.first >= 200,
      'events look buffered: ' + arrivals.toString());
  expect(types.contains('message_start') && types.contains('message_end'),
      'LMP stream must frame the message, saw ' + types.toString());

  final rawPayload = <String, dynamic>{
    'model': 'gpt-4o-mini',
    'messages': [
      {'role': 'user', 'content': 'hi'},
    ],
    'stream': true,
  };
  final native = NativeClient();
  final session = native.streamRaw(
      'openai_chat_completions', base, 'sk-test', 'gpt-4o-mini', rawPayload,
      timeoutMs: 20000, idleTimeoutMs: 20000);
  final rawClock = Stopwatch()..start();
  final rawArrivals = <int>[];
  var frames = 0;
  var sawDone = false;
  try {
    while (true) {
      final event = session.next();
      if (event == null) break;
      rawArrivals.add(rawClock.elapsedMilliseconds);
      final data = event['data'];
      expect(data is String && data.isNotEmpty,
          'native frames must carry the raw provider payload');
      expect(event.containsKey('event'), 'native frames must carry the wire event name');
      if (data == '[DONE]') {
        sawDone = true;
      } else {
        expect(jsonDecode(data as String) is Map, 'native payload must be JSON');
        frames++;
      }
    }
  } finally {
    session.close();
  }
  expect(frames == 2, 'native stream should deliver both provider frames, got ' + frames.toString());
  expect(sawDone, 'native stream should surface the [DONE] sentinel');
  expect(rawArrivals.length >= 2,
      'native stream delivered ' + rawArrivals.length.toString() + ' frames');
  expect(rawArrivals.last - rawArrivals.first >= 100,
      'native frames look buffered: ' + rawArrivals.toString());

  final cancelSession = StreamSession.open({
    'protocol': 'openai_chat_completions',
    'baseUrl': base,
    'apiKey': 'sk-test',
    'model': 'gpt-4o-mini',
    'payload': rawPayload,
  });
  try {
    expect(cancelSession.next() != null, 'first cancel-test event must arrive');
    cancelSession.cancel();
    var cancelled = false;
    try {
      while (cancelSession.next() != null) {
        // Drain until the cancellation surfaces.
      }
    } on CoreException catch (error) {
      cancelled = (error.code ?? '').contains('cancel');
    }
    expect(cancelled, 'cancelling a live stream must surface a cancelled error');
  } finally {
    cancelSession.close();
  }
  expect(cancelSession.closed, 'close must mark the session closed');
  cancelSession.close();
  var refusedAfterClose = false;
  try {
    cancelSession.next();
  } on StateError {
    refusedAfterClose = true;
  }
  expect(refusedAfterClose, 'a closed session must never touch the released handle');

  final buffered = client.streamAll('anthropic_messages', base, 'sk-ant', unifiedRequest);
  expect(buffered.any((event) => event['type'] == 'message_end'),
      'streamAll must return message_end');
  final openaiEvents = client
      .streamAll('openai_chat_completions', base, 'sk-test',
          UnifiedClient.textRequest('gpt-4o-mini', 'hi'));
  final openaiText = StringBuffer();
  var openaiTotal = 0;
  for (final event in openaiEvents) {
    if (event['type'] == 'text_delta') openaiText.write(event['delta']);
    if (event['type'] == 'usage') {
      openaiTotal = (event['usage'] as Map)['totalTokens'] as int;
    }
  }
  expect(openaiText.toString() == 'Hello world',
      'openai unified stream text was ' + openaiText.toString());
  expect(openaiTotal == 12, 'openai unified stream usage was ' + openaiTotal.toString());

  final single = client.stream('anthropic_messages', base, 'sk-ant', unifiedRequest);
  try {
    final first = single.next();
    expect(first != null, 'stream session must yield a first event');
  } finally {
    single.close();
  }
  native.close();
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

Future<void> securityChecks(UnifiedClient client, String base, Expect expect) async {
  var blocked = false;
  try {
    await client.completeText('cohere', base + '/redir', 'co-key', 'command-a', 'hi');
  } on HttpException catch (error) {
    blocked = error.message.contains('redirect');
  }
  expect(blocked, 'redirects must be blocked');

  var insecureRejected = false;
  try {
    CoreBridge.shared().prepareNative({
      'protocol': 'openai_chat_completions',
      'baseUrl': 'http://example.com/v1',
      'apiKey': 'k',
      'payload': const <String, dynamic>{},
    });
  } on CoreException catch (error) {
    insecureRejected = error.code == 'invalid_endpoint';
  }
  expect(insecureRejected, 'plain HTTP to a remote host must be rejected by the core');

  var missingKeyCode = '';
  try {
    CoreBridge.shared().prepareNative({
      'protocol': 'openai_chat_completions',
      'baseUrl': 'https://api.example.com/v1',
      'payload': const <String, dynamic>{},
    });
  } on CoreException catch (error) {
    missingKeyCode = error.code ?? '';
  }
  expect(missingKeyCode == 'auth_missing', 'missing API key code was ' + missingKeyCode);
}

// ---------------------------------------------------------------------------
// Catalog loader: offline cache, ETag/304, maxAge, stale fallback
// ---------------------------------------------------------------------------

Future<void> catalogLoaderChecks(String root, Expect expect) async {
  final fixture =
      await File(root + 'conformance/catalog/fixtures/v2_catalog.json').readAsBytes();
  var mode = 0;
  var hits = 0;
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((HttpRequest request) async {
    hits++;
    final response = request.response;
    if (request.uri.path != '/models.json') {
      response.statusCode = 404;
      await response.close();
      return;
    }
    if (mode == 0) {
      if (request.headers.value('if-none-match') == '"rev-1"') {
        response.statusCode = 304;
      } else {
        response.headers.set('etag', '"rev-1"');
        response.statusCode = 200;
        response.headers.contentType = ContentType.json;
        response.add(fixture);
      }
    } else {
      response.statusCode = 503;
    }
    await response.close();
  });

  final cacheDir = await Directory.systemTemp.createTemp('lingxi-dart-cache-');
  try {
    final url = 'http://127.0.0.1:' + server.port.toString() + '/models.json';
    final loader = CatalogLoader(CatalogLoaderConfig(
      endpoint: url,
      maxAge: const Duration(minutes: 30),
      cacheDirectory: cacheDir,
      requestTimeout: const Duration(seconds: 10),
      userAgent: 'Test/1.0',
    ));
    final first = await loader.load();
    expect(first.revision.catalogRevision == 'conf-rev-001', 'loader revision');
    expect(hits == 1, 'first load must hit the network once, got ' + hits.toString());
    expect(File(cacheDir.path + '/models.json').existsSync(), 'loader must persist models.json');
    expect(File(cacheDir.path + '/models.json.meta.json').existsSync(),
        'loader must persist meta.json');

    await loader.load();
    expect(hits == 1, 'fresh cache must skip the network, got ' + hits.toString());

    final revalidated = await loader.load(forceRefresh: true);
    expect(hits == 2, 'forceRefresh must revalidate, got ' + hits.toString());
    expect(revalidated.totalModels == 5, '304 must keep the cached catalog');

    mode = 1;
    final cold = CatalogLoader(CatalogLoaderConfig(
      endpoint: url,
      maxAge: Duration.zero,
      cacheDirectory: cacheDir,
      requestTimeout: const Duration(seconds: 10),
    ));
    final fallback = await cold.load();
    expect(fallback.totalModels == 5, 'stale disk cache must survive a 503');
    expect(hits == 3, 'cold loader must hit the network, got ' + hits.toString());

    final empty = CatalogLoader(CatalogLoaderConfig(
      endpoint: url,
      maxAge: Duration.zero,
      requestTimeout: const Duration(seconds: 10),
    ));
    var kind = '';
    var status = 0;
    try {
      await empty.load();
    } on CatalogException catch (error) {
      kind = error.kind.name;
      status = error.statusCode ?? 0;
    }
    expect(kind == 'unexpectedStatus', '503 without cache must raise unexpectedStatus');
    expect(status == 503, 'unexpectedStatus must carry the status code');
  } finally {
    await server.close(force: true);
    cacheDir.deleteSync(recursive: true);
  }
}

// ---------------------------------------------------------------------------
// Fixture server isolate
// ---------------------------------------------------------------------------

class FixtureServer {
  FixtureServer._(this._port, this._isolate, this._messages, this.baseUrl,
      this.requests);

  static const int _startupTimeoutMs = 10000;

  final ReceivePort _port;
  final Isolate _isolate;
  final StreamSubscription<Object?> _messages;
  final String baseUrl;

  /// Every request the server observed: path, authorization header, body.
  /// The isolate keeps appending to this same list, so it stays live.
  final List<Map<String, dynamic>> requests;

  static Future<FixtureServer> start(String root) async {
    final port = ReceivePort();
    final ready = Completer<int>();
    final startup = Completer<String>();
    final requests = <Map<String, dynamic>>[];
    final subscription = port.listen((message) {
      final map = (message as Map).cast<String, dynamic>();
      switch (map['type']) {
        case 'ready':
          if (!ready.isCompleted) ready.complete(map['port'] as int);
        case 'request':
          requests.add(map);
        case 'error':
          if (!startup.isCompleted) startup.complete(map['error'] as String);
      }
    });
    final isolate = await Isolate.spawn(_fixtureServerMain, [port.sendPort, root]);
    final resolved = await Future.any([
      ready.future,
      startup.future.then<int>((message) => throw StateError('fixture server: ' + message)),
    ]).timeout(const Duration(milliseconds: _startupTimeoutMs),
        onTimeout: () => throw StateError('fixture server did not start'));
    final server = FixtureServer._(port, isolate, subscription,
        'http://127.0.0.1:' + resolved.toString(), requests);
    return server;
  }

  Future<Map<String, dynamic>?> waitForRequest(
      bool Function(Map<String, dynamic> request) match) async {
    final deadline = DateTime.now().add(const Duration(seconds: 3));
    while (DateTime.now().isBefore(deadline)) {
      for (final request in requests) {
        if (match(request)) return request;
      }
      await Future<void>.delayed(const Duration(milliseconds: 10));
    }
    return null;
  }

  void stop() {
    _isolate.kill(priority: Isolate.immediate);
    _messages.cancel();
    _port.close();
  }
}

Future<void> _fixtureServerMain(List<Object?> args) async {
  final parent = args[0] as SendPort;
  final root = args[1] as String;
  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  parent.send({'type': 'ready', 'port': server.port});
  await for (final request in server) {
    unawaited(_handleFixture(request, root, parent));
  }
}

Future<void> _handleFixture(HttpRequest request, String root, SendPort parent) async {
  final path = request.uri.path;
  final body = await utf8.decoder.bind(request).join();
  parent.send({
    'type': 'request',
    'path': path,
    'auth': request.headers.value('authorization'),
    'body': body,
  });
  final response = request.response;
  try {
    if (path.startsWith('/redir')) {
      response.statusCode = 302;
      response.headers.set('location', 'http://127.0.0.1:9/leak');
      await response.close();
      return;
    }
    final streaming = body.contains('"stream":true');
    if (path.contains('converse')) {
      await _sendJson(response, 'aws_bedrock_response.json', root);
    } else if (path.contains('/v2/rerank')) {
      await _sendJson(response, 'cohere_rerank_response.json', root);
    } else if (path.contains('/embeddings')) {
      await _sendJson(response, 'openai_embed_response.json', root);
    } else if (path.contains(':generateContent')) {
      await _sendJson(response, 'google_gemini_response.json', root);
    } else if (path.contains('/messages') && streaming) {
      await _sendStream(response, 'anthropic_messages_stream.sse', root);
    } else if (path.contains('/messages')) {
      await _sendJson(response, 'anthropic_messages_response.json', root);
    } else if (path.contains('/chat/completions') && streaming) {
      await _sendStream(response, 'openai_chat_stream.sse', root);
    } else if (path.contains('/chat/completions')) {
      await _sendJson(response, 'openai_chat_response.json', root);
    } else {
      await _sendJson(response, 'cohere_chat_response.json', root);
    }
  } on Object catch (error) {
    parent.send({'type': 'error', 'error': path + ': ' + error.toString()});
    try {
      response.statusCode = 500;
      await response.close();
    } on Object {
      // The client may already have gone away.
    }
  }
}

Future<void> _sendJson(HttpResponse response, String fixture, String root) async {
  final bytes = await File(root + 'conformance/native/' + fixture).readAsBytes();
  response.statusCode = 200;
  response.headers.contentType = ContentType.json;
  response.add(bytes);
  await response.close();
}

/// Writes one SSE frame at a time with a visible gap, so a client that really
/// pulls incrementally sees the first frame long before the last one.
Future<void> _sendStream(HttpResponse response, String fixture, String root) async {
  final text = await File(root + 'conformance/native/' + fixture).readAsString();
  final frames = text
      .trim()
      .split(RegExp(r'\n\n'))
      .where((frame) => frame.trim().isNotEmpty)
      .toList();
  response.statusCode = 200;
  response.headers.contentType = ContentType('text', 'event-stream', charset: 'utf-8');
  response.bufferOutput = false;
  for (var i = 0; i < frames.length; i++) {
    response.write(frames[i]);
    response.write('\n\n');
    await response.flush();
    if (i < frames.length - 1) {
      await Future<void>.delayed(const Duration(milliseconds: 200));
    }
  }
  await response.close();
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

bool _same(Iterable<Object?> actual, Object? expected) {
  if (expected is! List) return false;
  final left = [for (final item in actual) item.toString()];
  final right = [for (final item in expected) item.toString()];
  if (left.length != right.length) return false;
  for (var i = 0; i < left.length; i++) {
    if (left[i] != right[i]) return false;
  }
  return true;
}

List<String> _strings(Object? value) =>
    value is List ? [for (final item in value) item.toString()] : const [];

String _textOf(Map<String, dynamic> response) {
  final buffer = StringBuffer();
  final blocks = response['content'];
  if (blocks is List) {
    for (final block in blocks) {
      if (block is Map && block['type'] == 'text') buffer.write(block['text']);
    }
  }
  return buffer.toString();
}

List<Map<String, dynamic>> _blocksOf(Map<String, dynamic> response, String type) {
  final out = <Map<String, dynamic>>[];
  final blocks = response['content'];
  if (blocks is List) {
    for (final block in blocks) {
      if (block is Map<String, dynamic> && block['type'] == type) out.add(block);
    }
  }
  return out;
}

Object? _headerValue(Map<String, dynamic> headers, String name) {
  for (final entry in headers.entries) {
    if (entry.key.toLowerCase() == name) return entry.value;
  }
  return null;
}

bool _catalogError(void Function() body, CatalogErrorKind kind) {
  try {
    body();
  } on CatalogException catch (error) {
    return error.kind == kind;
  }
  return false;
}
