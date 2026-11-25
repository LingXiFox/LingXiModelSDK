import 'dart:convert';

/// Model Catalog (schema 1.x / 2.x) decoding, ordering, lookup and filtering.
///
/// This module has no inference dependency: it only parses the published
/// `models.json` document, preserves unknown fields verbatim and exposes
/// three-state semantics (a missing value stays null; `0` and `false` are real
/// values). The semantics deliberately mirror the Swift/Java/Python reference
/// decoders so every SDK answers the conformance fixtures identically.

/// Lifecycle status of a catalog model.
enum ModelStatus {
  active,
  preview,
  deprecated,
  retired,
  unknown;

  /// Maps the raw `status` string. Deprecated and retired collapse into
  /// [ModelStatus.deprecated] because neither is selectable by default.
  static ModelStatus fromRaw(String? raw) {
    switch (raw?.trim().toLowerCase()) {
      case 'active':
        return ModelStatus.active;
      case 'beta':
      case 'preview':
        return ModelStatus.preview;
      case 'deprecated':
      case 'retired':
        return ModelStatus.deprecated;
      default:
        return ModelStatus.unknown;
    }
  }

  /// True when a default (non-deprecated) filter may return the model.
  bool get isSelectable => this != ModelStatus.deprecated;
}

/// Declared token limits, each optionally unknown.
class ModelLimits {
  const ModelLimits({this.context, this.input, this.output});

  final int? context;
  final int? input;
  final int? output;

  @override
  String toString() => 'ModelLimits(context: $context, input: $input, output: $output)';
}

/// Capability flags. [supportsTemperature] is three-state on purpose.
class ModelCapabilities {
  const ModelCapabilities({
    required this.reasoning,
    required this.reasoningEfforts,
    required this.toolCalling,
    required this.structuredOutput,
    required this.attachments,
    required this.vision,
    required this.audioInput,
    required this.audioOutput,
    required this.inputModalities,
    required this.outputModalities,
    required this.openWeights,
    required this.supportsTemperature,
  });

  final bool reasoning;
  final List<String> reasoningEfforts;
  final bool toolCalling;
  final bool structuredOutput;
  final bool attachments;
  final bool vision;
  final bool audioInput;
  final bool audioOutput;
  final List<String> inputModalities;
  final List<String> outputModalities;
  final bool openWeights;
  final bool? supportsTemperature;

  @override
  String toString() => 'ModelCapabilities(reasoning: $reasoning, toolCalling: $toolCalling)';
}

/// Per-million-token pricing. Every field is optional; [isUnknown] is true
/// only when the provider published no price at all at this granularity.
class ModelPricing {
  const ModelPricing({
    this.input,
    this.output,
    this.cacheRead,
    this.cacheWrite,
    this.reasoning,
    this.inputAudio,
    this.outputAudio,
  });

  final double? input;
  final double? output;
  final double? cacheRead;
  final double? cacheWrite;
  final double? reasoning;
  final double? inputAudio;
  final double? outputAudio;

  bool get isUnknown =>
      input == null &&
      output == null &&
      cacheRead == null &&
      cacheWrite == null &&
      reasoning == null &&
      inputAudio == null &&
      outputAudio == null;

  @override
  String toString() => 'ModelPricing(input: $input, output: $output)';
}

/// One model inside one provider.
class CatalogModel {
  CatalogModel({
    required this.id,
    required this.providerId,
    required this.name,
    this.family,
    this.overview,
    this.releaseDate,
    this.lastUpdated,
    this.knowledgeCutoff,
    this.canonicalModelId,
    required this.status,
    required this.limits,
    required this.capabilities,
    required this.pricing,
    required this.fields,
  });

  final String id;
  final String providerId;
  final String name;
  final String? family;
  final String? overview;
  final String? releaseDate;
  final String? lastUpdated;
  final String? knowledgeCutoff;
  final String? canonicalModelId;
  final ModelStatus status;
  final ModelLimits limits;
  final ModelCapabilities capabilities;
  final ModelPricing pricing;

  /// The raw JSON object, unknown fields included.
  final Map<String, dynamic> fields;

  /// `provider/model` identifier used throughout the SDKs.
  String get qualifiedId => '$providerId/$id';

  int? get contextWindow => limits.context;

  int? get maxOutputTokens => limits.output;

  @override
  String toString() => 'CatalogModel($qualifiedId, name: $name)';
}

/// One provider entry with its models in display order.
class CatalogProvider {
  CatalogProvider({
    required this.id,
    required this.name,
    this.baseUrl,
    this.documentationUrl,
    required this.environmentVariableNames,
    required this.models,
    required this.fields,
  });

  final String id;
  final String name;
  final String? baseUrl;
  final String? documentationUrl;
  final List<String> environmentVariableNames;
  final List<CatalogModel> models;

  /// The raw JSON object, unknown fields included.
  final Map<String, dynamic> fields;

  int get modelCount => models.length;

  @override
  String toString() => 'CatalogProvider($id, models: ${models.length})';
}

/// Document-level metadata.
class CatalogRevision {
  const CatalogRevision({
    required this.schemaVersion,
    required this.catalogRevision,
    this.catalogHash,
    this.source,
    this.sourceUrl,
    this.sourceHash,
    this.generatedAt,
    this.sourceFetchedAt,
    required this.totalProviders,
    required this.totalModels,
  });

  final String schemaVersion;
  final String catalogRevision;
  final String? catalogHash;
  final String? source;
  final String? sourceUrl;
  final String? sourceHash;
  final DateTime? generatedAt;
  final DateTime? sourceFetchedAt;
  final int totalProviders;
  final int totalModels;
}

/// Declarative model filter; every unset field means "do not constrain".
class ModelFilter {
  const ModelFilter({
    this.providerId,
    this.query,
    this.reasoning,
    this.toolCalling,
    this.structuredOutput,
    this.vision,
    this.attachments,
    this.openWeights,
    this.minimumContextWindow,
    this.minimumMaxOutputTokens,
    this.includeDeprecated = false,
    this.featuredProviderIds = const [],
  });

  final String? providerId;
  final String? query;
  final bool? reasoning;
  final bool? toolCalling;
  final bool? structuredOutput;
  final bool? vision;
  final bool? attachments;
  final bool? openWeights;
  final int? minimumContextWindow;
  final int? minimumMaxOutputTokens;
  final bool includeDeprecated;
  final List<String> featuredProviderIds;
}

/// Failure kinds of the catalog module.
enum CatalogErrorKind {
  invalidCatalog,
  unsupportedSchemaVersion,
  unexpectedStatus,
  transport,
  unavailable,
}

/// Catalog failure with the machine-readable [kind] plus the HTTP context when
/// there is one.
class CatalogException implements Exception {
  const CatalogException(this.kind, this.message, {this.statusCode, this.schemaVersion});

  final CatalogErrorKind kind;
  final String message;
  final int? statusCode;
  final String? schemaVersion;

  @override
  String toString() => 'CatalogException(${kind.name}): $message';
}

// ---------------------------------------------------------------------------
// Ordering
// ---------------------------------------------------------------------------

/// Natural ordering shared with the other SDKs: digit runs compare numerically
/// (so `gpt-5` precedes `gpt-10`), word runs compare lowercased.
int naturalCompare(String a, String b) {
  int compareRun(List<Object> left, List<Object> right) {
    final length = left.length < right.length ? left.length : right.length;
    for (var i = 0; i < length; i++) {
      final l = left[i];
      final r = right[i];
      int result;
      if (l is int && r is int) {
        result = l.compareTo(r);
      } else if (l is String && r is String) {
        result = l.compareTo(r);
      } else {
        result = l is int ? -1 : 1;
      }
      if (result != 0) return result;
    }
    return left.length.compareTo(right.length);
  }

  return compareRun(_tokens(a), _tokens(b));
}

List<Object> _tokens(String text) {
  final tokens = <Object>[];
  for (final match in RegExp(r'\d+|\D+').allMatches(text)) {
    final piece = match.group(0)!;
    if (RegExp(r'^\d+$').hasMatch(piece)) {
      final trimmed = piece.replaceFirst(RegExp(r'^0+(?=\d)'), '');
      tokens.addAll([0, trimmed.length, trimmed]);
    } else {
      tokens.addAll([1, 0, piece.toLowerCase()]);
    }
  }
  return tokens;
}

/// Providers sorted by featured rank, then natural name, then id.
List<CatalogProvider> sortProviders(
  Iterable<CatalogProvider> providers,
  List<String> featured,
) {
  final featuredRank = <String, int>{};
  for (var i = 0; i < featured.length; i++) {
    final key = featured[i].trim().toLowerCase();
    featuredRank.putIfAbsent(key, () => i);
  }
  final ranked = providers.toList();
  ranked.sort((a, b) {
    final aRank = featuredRank[a.id.trim().toLowerCase()] ?? featured.length;
    final bRank = featuredRank[b.id.trim().toLowerCase()] ?? featured.length;
    if (aRank != bRank) return aRank.compareTo(bRank);
    final byName = naturalCompare(a.name, b.name);
    if (byName != 0) return byName;
    return a.id.compareTo(b.id);
  });
  return ranked;
}

/// Models sorted newest release first, then family, name and id.
List<CatalogModel> modelsInStableOrder(Iterable<CatalogModel> models) {
  final ordered = models.toList();
  ordered.sort((a, b) {
    final aDate = _nonEmpty(a.releaseDate);
    final bDate = _nonEmpty(b.releaseDate);
    if (aDate == null && bDate != null) return 1;
    if (aDate != null && bDate == null) return -1;
    if (aDate != null && bDate != null) {
      final byDate = bDate.compareTo(aDate);
      if (byDate != 0) return byDate;
    }
    final byFamily = naturalCompare(a.family ?? '', b.family ?? '');
    if (byFamily != 0) return byFamily;
    final byName = naturalCompare(a.name, b.name);
    if (byName != 0) return byName;
    return a.id.compareTo(b.id);
  });
  return ordered;
}

// ---------------------------------------------------------------------------
// Decoder helpers
// ---------------------------------------------------------------------------

Map<String, dynamic>? _asObject(Object? value) =>
    value is Map<String, dynamic> ? value : null;

String? _optString(Object? value) => value is String ? value : null;

String? _nonEmpty(Object? value) {
  if (value is! String) return null;
  return value.trim().isEmpty ? null : value;
}

int? _optInt(Object? value) {
  if (value is int) return value;
  if (value is double && value == value.roundToDouble()) return value.toInt();
  return null;
}

double? _optDouble(Object? value) => value is num ? value.toDouble() : null;

bool? _optBool(Object? value) => value is bool ? value : null;

List<String> _stringList(Object? value) {
  if (value is! List) return const [];
  return [for (final item in value) if (item is String) item];
}

bool _hasPrefix(List<String> values, String prefix) =>
    values.any((value) => value.toLowerCase().startsWith(prefix));

DateTime? _parseTimestamp(String? text) {
  if (text == null || text.trim().isEmpty) return null;
  return DateTime.tryParse(text.trim())?.toUtc();
}

String _norm(Object? text) => text is String ? text.trim().toLowerCase() : '';

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

CatalogModel? _decodeModel(
  String providerId,
  Object? rawKey,
  Map<String, dynamic> fields,
) {
  final id = _nonEmpty(rawKey) ?? _nonEmpty(fields['id']);
  if (id == null) return null;

  final limits = _asObject(fields['limit']);
  final cost = _asObject(fields['cost']);
  final modalities = _asObject(fields['modalities']);
  final inputModalities = modalities == null ? const <String>[] : _stringList(modalities['input']);
  final outputModalities = modalities == null ? const <String>[] : _stringList(modalities['output']);
  final attachment = fields['attachment'] == true;

  final efforts = <String>[];
  final options = fields['reasoning_options'];
  if (options is List) {
    for (final option in options) {
      final object = _asObject(option);
      if (object != null) efforts.addAll(_stringList(object['values']));
    }
  }

  return CatalogModel(
    id: id,
    providerId: providerId,
    name: _optString(fields['name']) ?? id,
    family: _nonEmpty(fields['family']),
    overview: _nonEmpty(fields['description']),
    releaseDate: _nonEmpty(fields['release_date']),
    lastUpdated: _nonEmpty(fields['last_updated']),
    knowledgeCutoff: _nonEmpty(fields['knowledge']),
    canonicalModelId: _nonEmpty(fields['canonical_model_id']),
    status: ModelStatus.fromRaw(_optString(fields['status'])),
    limits: ModelLimits(
      context: limits == null ? null : _optInt(limits['context']),
      input: limits == null ? null : _optInt(limits['input']),
      output: limits == null ? null : _optInt(limits['output']),
    ),
    capabilities: ModelCapabilities(
      reasoning: fields['reasoning'] == true,
      reasoningEfforts: List.unmodifiable(efforts),
      toolCalling: fields['tool_call'] == true,
      structuredOutput: fields['structured_output'] == true,
      attachments: attachment,
      vision: attachment || _hasPrefix(inputModalities, 'image'),
      audioInput: _hasPrefix(inputModalities, 'audio'),
      audioOutput: _hasPrefix(outputModalities, 'audio'),
      inputModalities: List.unmodifiable(inputModalities),
      outputModalities: List.unmodifiable(outputModalities),
      openWeights: fields['open_weights'] == true,
      supportsTemperature: _optBool(fields['temperature']),
    ),
    pricing: ModelPricing(
      input: cost == null ? null : _optDouble(cost['input']),
      output: cost == null ? null : _optDouble(cost['output']),
      cacheRead: cost == null ? null : _optDouble(cost['cache_read']),
      cacheWrite: cost == null ? null : _optDouble(cost['cache_write']),
      reasoning: cost == null ? null : _optDouble(cost['reasoning']),
      inputAudio: cost == null ? null : _optDouble(cost['input_audio']),
      outputAudio: cost == null ? null : _optDouble(cost['output_audio']),
    ),
    fields: Map.unmodifiable(fields),
  );
}

CatalogProvider? _decodeProvider(Object? rawKey, Map<String, dynamic> fields) {
  final models = <CatalogModel>[];
  final rawModels = _asObject(fields['models']);
  if (rawModels != null) {
    for (final entry in rawModels.entries) {
      final modelFields = _asObject(entry.value);
      if (modelFields == null) continue;
      final model = _decodeModel(_optString(rawKey) ?? '$rawKey', entry.key, modelFields);
      if (model != null) models.add(model);
    }
  }
  if (models.isEmpty && !fields.containsKey('id')) return null;
  final id = _optString(fields['id']) ?? _optString(rawKey) ?? '$rawKey';
  final name = _optString(fields['name']) ?? id;
  return CatalogProvider(
    id: id,
    name: name,
    baseUrl: _nonEmpty(fields['baseURL']),
    documentationUrl: _nonEmpty(fields['doc']),
    environmentVariableNames: List.unmodifiable(_stringList(fields['env'])),
    models: List.unmodifiable(modelsInStableOrder(models)),
    fields: Map.unmodifiable(fields),
  );
}

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

/// Decoded, immutable model catalog.
class LingXiModelCatalog {
  LingXiModelCatalog._(this.revision, this.providers, this.rawDocument);

  final CatalogRevision revision;

  /// Providers in display order (featured first, then natural name).
  final List<CatalogProvider> providers;

  /// The exact decoded bytes, kept for caching and diagnostics.
  final List<int> rawDocument;

  /// Decodes `models.json` text; throws [CatalogException] on bad input.
  static LingXiModelCatalog decode(String jsonText) {
    late Object? parsed;
    try {
      parsed = jsonDecode(jsonText);
    } on FormatException catch (error) {
      throw CatalogException(
        CatalogErrorKind.invalidCatalog,
        'Invalid JSON: ${error.message}',
      );
    }
    final root = _asObject(parsed);
    if (root == null) {
      throw const CatalogException(
        CatalogErrorKind.invalidCatalog,
        'Catalog root is not an object',
      );
    }
    final providersObject = _asObject(root['providers']);
    if (providersObject == null || providersObject.isEmpty) {
      throw const CatalogException(
        CatalogErrorKind.invalidCatalog,
        'providers missing or empty',
      );
    }

    final schemaVersion = _optString(root['schemaVersion']) ??
        _optString(root['version']) ??
        '1.0';
    final major = schemaVersion.split('.').first;
    if (major != '1' && major != '2') {
      throw CatalogException(
        CatalogErrorKind.unsupportedSchemaVersion,
        'Unsupported schema version: $schemaVersion',
        schemaVersion: schemaVersion,
      );
    }

    final decoded = <CatalogProvider>[];
    for (final entry in providersObject.entries) {
      final providerFields = _asObject(entry.value);
      if (providerFields == null) continue;
      final provider = _decodeProvider(entry.key, providerFields);
      if (provider != null) decoded.add(provider);
    }
    if (decoded.isEmpty) {
      throw const CatalogException(
        CatalogErrorKind.invalidCatalog,
        'No valid provider entries',
      );
    }

    final ordered = sortProviders(decoded, const []);
    late final String revisionText;
    final generatedRaw = _optString(root['generatedAt']) ?? _optString(root['updatedAt']);
    final generatedAt = _parseTimestamp(generatedRaw);
    final rawRevision = _optString(root['catalogRevision']) ?? _optString(root['catalogHash']);
    if (rawRevision != null) {
      revisionText = rawRevision;
    } else if (generatedAt != null) {
      revisionText = generatedAt.toIso8601String();
    } else {
      revisionText = '';
    }

    final declaredProviders = _optInt(root['totalProviders']);
    final declaredModels = _optInt(root['totalModels']);
    final revision = CatalogRevision(
      schemaVersion: schemaVersion,
      catalogRevision: revisionText,
      catalogHash: _optString(root['catalogHash']),
      source: _optString(root['source']),
      sourceUrl: _optString(root['sourceURL']),
      sourceHash: _optString(root['sourceHash']),
      generatedAt: generatedAt,
      sourceFetchedAt: _parseTimestamp(_optString(root['sourceFetchedAt'])),
      totalProviders: declaredProviders ?? ordered.length,
      totalModels: declaredModels ??
          ordered.fold<int>(0, (sum, provider) => sum + provider.modelCount),
    );
    return LingXiModelCatalog._(
      revision,
      List.unmodifiable(ordered),
      List.unmodifiable(utf8.encode(jsonText)),
    );
  }

  /// Sum of models across providers (recomputed, never trusted blindly).
  int get totalModels =>
      providers.fold<int>(0, (sum, provider) => sum + provider.modelCount);

  /// Case-insensitive, trimmed provider lookup.
  CatalogProvider? provider(String providerId) {
    final key = _norm(providerId);
    for (final entry in providers) {
      if (_norm(entry.id) == key) return entry;
    }
    return null;
  }

  /// Model lookup by provider + model id, or by `provider/model`.
  CatalogModel? model(String providerIdOrQualified, [String? modelId]) {
    if (modelId != null) {
      final provider = this.provider(providerIdOrQualified);
      if (provider == null) return null;
      final key = _norm(modelId);
      for (final model in provider.models) {
        if (_norm(model.id) == key) return model;
      }
      return null;
    }
    final slash = providerIdOrQualified.indexOf('/');
    if (slash >= 0) {
      return model(
        providerIdOrQualified.substring(0, slash),
        providerIdOrQualified.substring(slash + 1),
      );
    }
    final key = _norm(providerIdOrQualified);
    for (final provider in providers) {
      for (final model in provider.models) {
        if (_norm(model.id) == key) return model;
      }
    }
    return null;
  }

  /// Providers ordered with [featured] pinned to the front.
  List<CatalogProvider> sortedProviders([List<String> featured = const []]) =>
      sortProviders(providers, featured);

  /// Models of one provider in display order.
  List<CatalogModel> modelsForProvider(String providerId) {
    final provider = this.provider(providerId);
    if (provider == null) return const [];
    return modelsInStableOrder(provider.models);
  }

  /// Every model, provider by provider, in display order.
  List<CatalogModel> allModels() {
    final out = <CatalogModel>[];
    for (final provider in sortedProviders()) {
      out.addAll(modelsInStableOrder(provider.models));
    }
    return List.unmodifiable(out);
  }

  /// Substring search over provider and model text; a provider whose own
  /// name/id matches contributes all of its models.
  List<CatalogModel> search(String query, [int? limit]) {
    final needle = _norm(query);
    if (needle.isEmpty) {
      final all = allModels();
      if (limit != null && all.length > limit) return all.sublist(0, limit);
      return all;
    }
    final matches = <CatalogModel>[];
    for (final provider in sortedProviders()) {
      final ordered = modelsInStableOrder(provider.models);
      if (_norm(provider.id).contains(needle) || _norm(provider.name).contains(needle)) {
        matches.addAll(ordered);
      } else {
        for (final model in ordered) {
          if (_containsAny(needle, [model.id, model.name, model.family, model.overview])) {
            matches.add(model);
          }
        }
      }
      if (limit != null && matches.length >= limit) {
        return List.unmodifiable(matches.sublist(0, limit));
      }
    }
    return List.unmodifiable(matches);
  }

  /// Models matching every constraint in [filter].
  List<CatalogModel> modelsMatching(ModelFilter filter) {
    final matches = <CatalogModel>[];
    for (final provider in sortedProviders(filter.featuredProviderIds)) {
      if (filter.providerId != null && _norm(provider.id) != _norm(filter.providerId)) {
        continue;
      }
      for (final model in modelsInStableOrder(provider.models)) {
        if (_matches(model, provider, filter)) matches.add(model);
      }
    }
    return List.unmodifiable(matches);
  }

  static bool _matches(
    CatalogModel model,
    CatalogProvider provider,
    ModelFilter filter,
  ) {
    if (!filter.includeDeprecated && !model.status.isSelectable) return false;
    final capabilities = model.capabilities;
    if (filter.reasoning != null && capabilities.reasoning != filter.reasoning) {
      return false;
    }
    if (filter.toolCalling != null && capabilities.toolCalling != filter.toolCalling) {
      return false;
    }
    if (filter.structuredOutput != null &&
        capabilities.structuredOutput != filter.structuredOutput) {
      return false;
    }
    if (filter.vision != null && capabilities.vision != filter.vision) return false;
    if (filter.attachments != null && capabilities.attachments != filter.attachments) {
      return false;
    }
    if (filter.openWeights != null && capabilities.openWeights != filter.openWeights) {
      return false;
    }
    if (filter.minimumContextWindow != null &&
        (model.limits.context ?? 0) < filter.minimumContextWindow!) {
      return false;
    }
    if (filter.minimumMaxOutputTokens != null &&
        (model.limits.output ?? 0) < filter.minimumMaxOutputTokens!) {
      return false;
    }
    final query = filter.query;
    if (query != null) {
      final needle = _norm(query);
      if (needle.isNotEmpty &&
          !_containsAny(needle, [
            model.id,
            model.name,
            model.family,
            model.overview,
            provider.id,
            provider.name,
          ])) {
        return false;
      }
    }
    return true;
  }

  static bool _containsAny(String needle, Iterable<String?> values) =>
      values.any((value) => _norm(value).contains(needle));
}
