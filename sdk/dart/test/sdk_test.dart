import 'dart:io';
import '../lib/catalog.dart';
import '../lib/registry.dart';
import '../lib/unified.dart';

Future<void> main() async {
  final cat = ModelCatalog.decode('{"schemaVersion":"2.0","providers":{"openai":{"id":"openai"}}}');
  if (!cat.providers.containsKey('openai')) throw AssertionError('Catalog decode failed');

  final regText = await File('../../registry/registry.json').readAsString();
  final reg = ProviderRegistry.fromJson(regText);
  if (reg.length < 320) throw AssertionError('Expected >=320 providers, got ${reg.length}');
  final ep = reg.resolveEndpoint('cohere', 'cohere');
  if (!ep.contains('cohere.com')) throw AssertionError('Unexpected endpoint: $ep');

  final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
  server.listen((HttpRequest req) async {
    final path = req.uri.path;
    if (path.startsWith('/redir')) {
      req.response.statusCode = 302;
      req.response.headers.set('Location', 'http://127.0.0.1:9/leak');
      await req.response.close();
      return;
    }
    final fixture = path.contains('/v2/rerank') ? 'cohere_rerank_response.json' : 'cohere_chat_response.json';
    final content = await File('../../conformance/native/$fixture').readAsString();
    req.response.statusCode = 200;
    req.response.headers.contentType = ContentType.json;
    req.response.write(content);
    await req.response.close();
  });

  final client = UnifiedClient();
  try {
    final base = 'http://127.0.0.1:${server.port}';
    final chat = await client.completeText('cohere', base, 'co-key', 'command-a', 'hi');
    if (chat['finish_reason'] != 'COMPLETE') throw AssertionError('Unexpected cohere finish_reason');

    final rr = await client.rerank('cohere', base, 'co-key', 'rerank-v3.5', 'dart', ['a', 'b']);
    if ((rr['results'] as List).isEmpty) throw AssertionError('Missing rerank results');

    var threwUnsupported = false;
    try {
      await client.rerank('openai_chat_completions', base, 'sk', 'm', 'q', ['a']);
    } on UnsupportedFeatureException {
      threwUnsupported = true;
    }
    if (!threwUnsupported) throw AssertionError('Expected UnsupportedFeatureException');

    var threwRedirect = false;
    try {
      await client.completeText('cohere', '$base/redir', 'co-key', 'command-a', 'hi');
    } on HttpException {
      threwRedirect = true;
    }
    if (!threwRedirect) throw AssertionError('Expected redirect block');

    print('Dart 4-module SDK tests passed!');
  } finally {
    client.nativeClient.close();
    await server.close(force: true);
  }
}
