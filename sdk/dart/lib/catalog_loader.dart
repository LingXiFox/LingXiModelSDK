import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'catalog.dart';

/// HTTP reply used by the catalog loader, mirroring the other SDKs.
class CatalogHttpReply {
  const CatalogHttpReply(this.statusCode, this.body, {this.etag});

  final int statusCode;
  final List<int> body;
  final String? etag;
}

/// Pluggable transport so tests never touch the network.
abstract class CatalogTransport {
  Future<CatalogHttpReply> fetch(
    String url,
    Map<String, String> headers,
    Duration timeout,
  );
}

/// Default transport: plain GET, no redirects, hard timeout.
class HttpCatalogTransport implements CatalogTransport {
  HttpCatalogTransport({HttpClient? client})
      : _client = client ?? (HttpClient()..connectionTimeout = const Duration(seconds: 30));

  final HttpClient _client;

  @override
  Future<CatalogHttpReply> fetch(
    String url,
    Map<String, String> headers,
    Duration timeout,
  ) async {
    final request = await _client.getUrl(Uri.parse(url)).timeout(timeout);
    request.followRedirects = false;
    headers.forEach(request.headers.set);
    final response = await request.close().timeout(timeout);
    final builder = BytesBuilder(copy: false);
    await response.timeout(timeout).forEach(builder.add);
    return CatalogHttpReply(
      response.statusCode,
      builder.takeBytes(),
      etag: response.headers.value('etag'),
    );
  }
}

/// Loader configuration; defaults match the Java/Python SDKs.
class CatalogLoaderConfig {
  const CatalogLoaderConfig({
    this.endpoint = defaultEndpoint,
    this.maxAge = const Duration(minutes: 30),
    this.cacheDirectory,
    this.requestTimeout = const Duration(seconds: 30),
    this.userAgent = 'LingXiModelSDK-Dart/2.0',
  });

  static const String defaultEndpoint = 'https://models.lingxifox.cn/models.json';

  final String endpoint;
  final Duration maxAge;
  final Directory? cacheDirectory;
  final Duration requestTimeout;
  final String userAgent;
}

/// Catalog plus the freshness/ETag metadata kept alongside it.
class CachedCatalog {
  const CachedCatalog(this.catalog, this.fetchedAt, this.etag);

  final LingXiModelCatalog catalog;
  final DateTime fetchedAt;
  final String? etag;

  /// True while the cache is younger than [maxAge]; a zero or negative
  /// maxAge always forces a revalidation.
  bool isFresh(Duration maxAge, DateTime now) {
    if (maxAge <= Duration.zero) return false;
    return now.difference(fetchedAt) < maxAge;
  }
}

/// Loads `models.json` with an offline cache, ETag revalidation and stale
/// fallback, matching the Java reference behaviour.
class CatalogLoader {
  CatalogLoader(this.config, {CatalogTransport? transport})
      : transport = transport ?? HttpCatalogTransport();

  final CatalogLoaderConfig config;
  final CatalogTransport transport;

  CachedCatalog? _memory;

  /// Last known catalog: memory first, then the offline cache file. Returns
  /// null when nothing usable is on disk.
  CachedCatalog? cached() {
    final memory = _memory;
    if (memory != null) return memory;
    final directory = config.cacheDirectory;
    if (directory == null) return null;
    final catalogFile = File('${directory.path}${Platform.pathSeparator}models.json');
    if (!catalogFile.existsSync()) return null;
    try {
      final raw = catalogFile.readAsStringSync();
      final catalog = LingXiModelCatalog.decode(raw);
      String? cachedEtag;
      var fetchedAt = catalog.revision.generatedAt ?? DateTime.fromMillisecondsSinceEpoch(0, isUtc: true);
      final metaFile =
          File('${directory.path}${Platform.pathSeparator}models.json.meta.json');
      if (metaFile.existsSync()) {
        final meta = jsonDecode(metaFile.readAsStringSync());
        if (meta is Map) {
          final rawEtag = meta['etag'];
          if (rawEtag is String) cachedEtag = rawEtag;
          final rawFetched = meta['fetchedAt'];
          if (rawFetched is String) {
            final parsed = DateTime.tryParse(rawFetched);
            if (parsed != null) fetchedAt = parsed.toUtc();
          }
        }
      }
      final cached = CachedCatalog(catalog, fetchedAt, cachedEtag);
      _memory = cached;
      return cached;
    } on Object {
      return null;
    }
  }

  /// Loads the catalog. A fresh cache skips the network entirely; `forceRefresh`
  /// revalidates with `If-None-Match` and accepts a 304.
  Future<LingXiModelCatalog> load({bool forceRefresh = false}) async {
    final cached = this.cached();
    final now = DateTime.now().toUtc();
    if (!forceRefresh && cached != null && cached.isFresh(config.maxAge, now)) {
      return cached.catalog;
    }

    final headers = <String, String>{
      'Accept': 'application/json',
      'User-Agent': config.userAgent,
    };
    final cachedEtag = cached?.etag;
    if (cachedEtag != null && cachedEtag.isNotEmpty) {
      headers['If-None-Match'] = cachedEtag;
    }

    final CatalogHttpReply reply;
    try {
      reply = await transport.fetch(config.endpoint, headers, config.requestTimeout);
    } on Object catch (error) {
      if (cached != null) return cached.catalog;
      final message = '${error.toString()}';
      throw CatalogException(
        CatalogErrorKind.transport,
        message,
      );
    }

    if (reply.statusCode == 304 && cached != null) {
      final etag = reply.etag ?? cached.etag;
      final fetchedAt = DateTime.now().toUtc();
      _memory = CachedCatalog(cached.catalog, fetchedAt, etag);
      _persistMeta(etag, fetchedAt);
      return cached.catalog;
    }

    if (reply.statusCode < 200 || reply.statusCode > 299) {
      if (cached != null) return cached.catalog;
      throw CatalogException(
        CatalogErrorKind.unexpectedStatus,
        'Unexpected HTTP status: ${reply.statusCode}',
        statusCode: reply.statusCode,
      );
    }

    try {
      final catalog = LingXiModelCatalog.decode(utf8.decode(reply.body));
      final fetchedAt = DateTime.now().toUtc();
      _memory = CachedCatalog(catalog, fetchedAt, reply.etag);
      _persistCatalog(reply.body, reply.etag, fetchedAt);
      return catalog;
    } on CatalogException {
      if (cached != null) return cached.catalog;
      rethrow;
    }
  }

  void _persistCatalog(List<int> raw, String? etag, DateTime fetchedAt) {
    final directory = config.cacheDirectory;
    if (directory == null) return;
    _writeAtomic(
      File('${directory.path}${Platform.pathSeparator}models.json'),
      raw,
    );
    _persistMeta(etag, fetchedAt);
  }

  void _persistMeta(String? etag, DateTime fetchedAt) {
    final directory = config.cacheDirectory;
    if (directory == null) return;
    final meta = jsonEncode(<String, Object?>{
      'etag': etag,
      'fetchedAt': fetchedAt.toIso8601String(),
    });
    _writeAtomic(
      File('${directory.path}${Platform.pathSeparator}models.json.meta.json'),
      utf8.encode(meta),
    );
  }

  /// Writes through a temp file in the same directory, then renames, so a
  /// concurrent reader never observes a half-written cache.
  void _writeAtomic(File target, List<int> data) {
    try {
      target.parent.createSync(recursive: true);
      final temp = File('${target.path}.tmp-${pid}-${DateTime.now().microsecondsSinceEpoch}');
      temp.writeAsBytesSync(data, flush: true);
      temp.renameSync(target.path);
    } on FileSystemException {
      // Caching is best effort; a read-only cache directory must not fail loads.
    }
  }
}
