import 'dart:convert';

class ProviderRegistry {
  static const List<String> allProtocols = [
    'openai_chat_completions',
    'openai_responses',
    'anthropic_messages',
    'google_gemini',
    'aws_bedrock',
    'google_vertex_ai',
    'azure_openai',
    'ollama',
    'cohere',
  ];

  final Map<String, dynamic> providers;
  ProviderRegistry(this.providers);

  static ProviderRegistry fromJson(String jsonText) {
    final doc = jsonDecode(jsonText) as Map<String, dynamic>;
    if (doc['schemaVersion'] != '1.0') {
      throw FormatException('Unsupported registry schemaVersion: ${doc['schemaVersion']}');
    }
    return ProviderRegistry((doc['providers'] as Map<String, dynamic>?) ?? {});
  }

  int get length => providers.length;

  String resolveEndpoint(String providerId, String protocol, {String? endpointOverride, bool allowInsecureHttp = false}) {
    final entry = providers[providerId] as Map<String, dynamic>?;
    if (entry == null) throw ArgumentError('Unknown provider: $providerId');
    final protos = (entry['protocols'] as List<dynamic>? ?? []).cast<String>();
    if (!protos.contains(protocol)) {
      throw StateError("Provider '$providerId' does not declare protocol '$protocol'");
    }
    final endpoints = (entry['officialEndpoints'] as Map<String, dynamic>?) ?? {};
    final raw = (endpointOverride != null && endpointOverride.isNotEmpty) ? endpointOverride : endpoints[protocol] as String?;
    if (raw == null || raw.isEmpty) throw StateError('No endpoint for $providerId/$protocol');
    final clean = raw.replaceAll(RegExp(r'/+$'), '');
    validateEndpointUrl(clean, allowInsecureHttp: allowInsecureHttp);
    return clean;
  }

  static void validateEndpointUrl(String rawUrl, {bool allowInsecureHttp = false}) {
    final uri = Uri.parse(rawUrl);
    if (uri.scheme == 'https' && uri.host.isNotEmpty) return;
    if (uri.scheme == 'http' && uri.host.isNotEmpty) {
      final h = uri.host.toLowerCase();
      if (allowInsecureHttp || h == '127.0.0.1' || h == 'localhost' || h == '::1') return;
      throw ArgumentError('Non-HTTPS endpoint disallowed: $rawUrl');
    }
    throw ArgumentError('Unsupported endpoint URL: $rawUrl');
  }
}
