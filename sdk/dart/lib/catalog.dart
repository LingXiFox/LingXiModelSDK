import 'dart:convert';

/// Zero-inference-dependency Model Catalog decoder for schema 2.0.
class ModelCatalog {
  final String schemaVersion;
  final Map<String, dynamic> providers;

  ModelCatalog._(this.schemaVersion, this.providers);

  static ModelCatalog decode(String jsonText) {
    final doc = jsonDecode(jsonText);
    if (doc is! Map<String, dynamic> || doc['schemaVersion'] != '2.0') {
      throw FormatException('Unsupported or invalid catalog schemaVersion (expected 2.0)');
    }
    final providers = (doc['providers'] as Map<String, dynamic>?) ?? <String, dynamic>{};
    return ModelCatalog._('2.0', providers);
  }
}
