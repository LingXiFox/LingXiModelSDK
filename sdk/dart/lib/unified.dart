import 'native.dart';

class UnsupportedFeatureException implements Exception {
  final String protocol;
  final String feature;
  UnsupportedFeatureException(this.protocol, this.feature);
  @override
  String toString() => "Unsupported feature '$feature' for protocol '$protocol'";
}

class UnifiedClient {
  final NativeClient nativeClient;
  UnifiedClient([NativeClient? nativeClient]) : nativeClient = nativeClient ?? NativeClient();

  Future<Map<String, dynamic>> completeText(
    String protocol,
    String baseUrl,
    String? apiKey,
    String model,
    String prompt,
  ) async {
    final cleanBase = baseUrl.replaceAll(RegExp(r'/+$'), '');
    late String uri;
    late Map<String, dynamic> payload;
    switch (protocol) {
      case 'openai_chat_completions':
        uri = '/chat/completions';
        payload = {'model': model, 'messages': [{'role': 'user', 'content': prompt}]};
        break;
      case 'openai_responses':
        uri = '/responses';
        payload = {'model': model, 'input': [{'role': 'user', 'content': [{'type': 'input_text', 'text': prompt}]}]};
        break;
      case 'anthropic_messages':
        uri = cleanBase.endsWith('/v1') ? '/messages' : '/v1/messages';
        payload = {'model': model, 'max_tokens': 1024, 'messages': [{'role': 'user', 'content': [{'type': 'text', 'text': prompt}]}]};
        break;
      case 'google_gemini':
        uri = '/v1beta/models/$model:generateContent';
        payload = {'contents': [{'role': 'user', 'parts': [{'text': prompt}]}]};
        break;
      case 'aws_bedrock':
        uri = '/model/$model/converse';
        payload = {'messages': [{'role': 'user', 'content': [{'text': prompt}]}]};
        break;
      case 'google_vertex_ai':
        uri = '/v1/publishers/google/models/$model:generateContent';
        payload = {'contents': [{'role': 'user', 'parts': [{'text': prompt}]}]};
        break;
      case 'azure_openai':
        uri = '/openai/deployments/$model/chat/completions?api-version=2024-10-21';
        payload = {'model': model, 'messages': [{'role': 'user', 'content': prompt}]};
        break;
      case 'ollama':
        uri = '/api/chat';
        payload = {'model': model, 'stream': false, 'messages': [{'role': 'user', 'content': prompt}]};
        break;
      case 'cohere':
        uri = '/v2/chat';
        payload = {'model': model, 'messages': [{'role': 'user', 'content': prompt}]};
        break;
      default:
        throw UnsupportedFeatureException(protocol, 'protocol');
    }
    return nativeClient.postJson('$cleanBase$uri', nativeClient.buildHeaders(protocol, apiKey), payload);
  }

  Future<Map<String, dynamic>> rerank(
    String protocol,
    String baseUrl,
    String apiKey,
    String model,
    String query,
    List<String> documents,
  ) async {
    if (protocol != 'cohere') {
      throw UnsupportedFeatureException(protocol, 'rerank');
    }
    return nativeClient.postJson(
      '${baseUrl.replaceAll(RegExp(r'/+$'), '')}/v2/rerank',
      {'Authorization': 'Bearer $apiKey'},
      {'model': model, 'query': query, 'documents': documents},
    );
  }
}
