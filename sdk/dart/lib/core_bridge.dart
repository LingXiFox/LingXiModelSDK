import 'dart:convert';
import 'dart:ffi';
import 'dart:io';

/// Failure reported by the shared Rust core (or by the bindings themselves).
///
/// [message] already carries the core operation and the core's error text;
/// [code] is the machine-readable code when the core reported one
/// (for example `invalid_endpoint`, `auth_missing` or `unsupported_feature`).
class CoreException implements Exception {
  final String message;
  final String? code;

  const CoreException(this.message, [this.code]);

  @override
  String toString() =>
      code == null ? 'CoreException: $message' : 'CoreException [$code]: $message';
}

typedef _Call1Native = Pointer<Uint8> Function(Pointer<Uint8>);
typedef _Call1 = Pointer<Uint8> Function(Pointer<Uint8>);
typedef _Call2Native = Pointer<Uint8> Function(Pointer<Uint8>, Pointer<Uint8>);
typedef _Call2 = Pointer<Uint8> Function(Pointer<Uint8>, Pointer<Uint8>);
typedef _Call0 = Pointer<Uint8> Function();
typedef _OpenHandleNative = Pointer<Void> Function(Pointer<Uint8>);
typedef _OpenHandle = Pointer<Void> Function(Pointer<Uint8>);
typedef _NextNative = Pointer<Uint8> Function(Pointer<Void>, Uint64);
typedef _Next = Pointer<Uint8> Function(Pointer<Void>, int);
typedef _HandleVoidNative = Void Function(Pointer<Void>);
typedef _HandleVoid = void Function(Pointer<Void>);
typedef _HandleStringNative = Pointer<Uint8> Function(Pointer<Void>);
typedef _HandleString = Pointer<Uint8> Function(Pointer<Void>);
typedef _FreeNative = Void Function(Pointer<Uint8>);
typedef _Free = void Function(Pointer<Uint8>);
typedef _MallocNative = Pointer<Uint8> Function(IntPtr);
typedef _Malloc = Pointer<Uint8> Function(int);

/// Dart FFI bindings for the Rust core declared in `core/include/lingxi_core.h`.
///
/// Every protocol decision (endpoint joining, provider auth, AWS SigV4, payload
/// translation, response and stream normalization) lives in Rust. This class
/// only moves UTF-8 JSON across the C ABI, frees every returned string with
/// `lingxi_string_free` and turns `{"ok":false,...}` envelopes into
/// [CoreException].
class CoreBridge {
  /// File name of the cdylib for the current host platform.
  static String get platformLibraryName {
    if (Platform.isMacOS) return 'liblingxi_core.dylib';
    if (Platform.isWindows) return 'lingxi_core.dll';
    return 'liblingxi_core.so';
  }

  final String libraryPath;
  final DynamicLibrary _library;

  late final _Call2 _translateRequest;
  late final _Call2 _normalizeResponse;
  late final _Call2 _normalizeStreamEvents;
  late final _Call1 _nativePrepare;
  late final _Call1 _unifiedPrepare;
  late final _Call1 _embed;
  late final _Call1 _rerank;
  late final _OpenHandle _streamOpen;
  late final _Call0 _streamOpenError;
  late final _Next _streamNext;
  late final _HandleVoid _streamCancel;
  late final _HandleVoid _streamClose;
  late final _HandleString _streamLastError;
  late final _Call1 _stringFree;

  final _Malloc _malloc;
  final _Free _free;

  static CoreBridge? _shared;

  CoreBridge._(this.libraryPath, DynamicLibrary library, DynamicLibrary libc)
      : _library = library,
        _malloc = libc.lookupFunction<_MallocNative, _Malloc>('malloc'),
        _free = libc.lookupFunction<_FreeNative, _Free>('free') {
    _translateRequest = _lookup2('lingxi_lmp_translate_request');
    _normalizeResponse = _lookup2('lingxi_lmp_normalize_response');
    _normalizeStreamEvents = _lookup2('lingxi_lmp_normalize_stream_events');
    _nativePrepare = _lookup1('lingxi_native_prepare');
    _unifiedPrepare = _lookup1('lingxi_unified_prepare');
    _embed = _lookup1('lingxi_unified_embed');
    _rerank = _lookup1('lingxi_unified_rerank');
    _streamOpen = _library.lookupFunction<_OpenHandleNative, _OpenHandle>(
        'lingxi_stream_open');
    _streamOpenError =
        _library.lookupFunction<_Call0, _Call0>('lingxi_stream_open_error');
    _streamNext = _library.lookupFunction<_NextNative, _Next>('lingxi_stream_next');
    _streamCancel =
        _library.lookupFunction<_HandleVoidNative, _HandleVoid>('lingxi_stream_cancel');
    _streamClose =
        _library.lookupFunction<_HandleVoidNative, _HandleVoid>('lingxi_stream_close');
    _streamLastError = _library
        .lookupFunction<_HandleStringNative, _HandleString>('lingxi_stream_last_error');
    _stringFree = _library.lookupFunction<_Call1Native, _Call1>('lingxi_string_free');
  }

  /// The process-wide bridge; the cdylib is opened once and reused.
  static CoreBridge shared() => _shared ??= _open();

  /// True when the core library can be located and loaded.
  static bool get available {
    if (_shared != null) return true;
    try {
      shared();
      return true;
    } catch (_) {
      return false;
    }
  }

  /// Where the core library was loaded from (or where we looked for it).
  static String? get loadedLibraryPath => _shared?.libraryPath;

  /// Candidate paths, in the same order the other SDKs use: the
  /// `LINGXI_CORE_LIB` override first, then `core/target/{release,debug}`
  /// walking up from the current directory.
  static List<String> candidatePaths() {
    final candidates = <String>[];
    final override = Platform.environment['LINGXI_CORE_LIB'];
    if (override != null && override.isNotEmpty) candidates.add(override);
    var directory = Directory.current;
    while (true) {
      for (final profile in const ['release', 'debug']) {
        candidates.add('${directory.path}${Platform.pathSeparator}core'
            '${Platform.pathSeparator}target${Platform.pathSeparator}$profile'
            '${Platform.pathSeparator}$platformLibraryName');
      }
      final parent = directory.parent;
      if (parent.path == directory.path) break;
      directory = parent;
    }
    return candidates;
  }

  /// First candidate that exists as a regular file, or null.
  static String? locateLibrary() {
    for (final candidate in candidatePaths()) {
      final file = File(candidate);
      if (file.existsSync()) return file.path;
    }
    return null;
  }

  static CoreBridge _open() {
    final override = Platform.environment['LINGXI_CORE_LIB'];
    final path = locateLibrary();
    if (path == null) {
      throw CoreException(
        'could not locate $platformLibraryName (set LINGXI_CORE_LIB or build the '
        'core with: cargo build --manifest-path core/Cargo.toml). Looked in: '
        '${candidatePaths().join(', ')}',
        'core_unavailable',
      );
    }
    if (override != null && override.isNotEmpty && path == override) {
      // The override must be a regular file, matching the other SDKs.
      if (!File(override).existsSync()) {
        throw CoreException('LINGXI_CORE_LIB is not a file: $override', 'core_unavailable');
      }
    }
    try {
      return CoreBridge._(path, DynamicLibrary.open(path), _libc());
    } on Object catch (error) {
      throw CoreException('could not load $path: $error', 'core_unavailable');
    }
  }

  /// libc handle used for the malloc/free pair that bridges Dart strings to
  /// the C ABI. `DynamicLibrary.process()` resolves libc on macOS and Linux.
  static DynamicLibrary _libc() {
    try {
      return DynamicLibrary.process();
    } on Object {
      if (Platform.isMacOS) return DynamicLibrary.open('/usr/lib/libSystem.B.dylib');
      if (Platform.isWindows) return DynamicLibrary.open('msvcrt.dll');
      return DynamicLibrary.open('libc.so.6');
    }
  }

  _Call1 _lookup1(String symbol) => _library.lookupFunction<_Call1Native, _Call1>(symbol);

  _Call2 _lookup2(String symbol) => _library.lookupFunction<_Call2Native, _Call2>(symbol);

  // ---------------------------------------------------------------------------
  // LMP translation and normalization
  // ---------------------------------------------------------------------------

  /// Provider-native request payload for [request] (an LMP request map).
  Map<String, dynamic> translateRequest(
      String protocol, Map<String, dynamic> request) {
    final envelope = _call2(_translateRequest, 'lingxi_lmp_translate_request',
        protocol, jsonEncode(request));
    return _asMap(envelope['payload'], 'translate_request payload');
  }

  /// Normalizes a provider-native response body into an LMP response map.
  Map<String, dynamic> normalizeResponse(String protocol, Map<String, dynamic> raw) {
    final envelope = _call2(_normalizeResponse, 'lingxi_lmp_normalize_response',
        protocol, jsonEncode(raw));
    return _asMap(envelope['response'], 'normalize_response response');
  }

  /// Normalizes a list of provider frames (`{event,data,id}`) into LMP stream
  /// events.
  List<Map<String, dynamic>> normalizeStreamEvents(
      String protocol, List<Map<String, dynamic>> events) {
    final envelope = _call2(_normalizeStreamEvents, 'lingxi_lmp_normalize_stream_events',
        protocol, jsonEncode(events));
    final list = envelope['events'];
    if (list is! List) {
      throw CoreException('normalize_stream_events returned no events list', 'invalid_envelope');
    }
    return [for (final item in list) _asMap(item, 'normalize_stream_events event')];
  }

  // ---------------------------------------------------------------------------
  // Prepare / embed / rerank
  // ---------------------------------------------------------------------------

  /// Resolves url, auth headers, body and framing for one native request.
  ///
  /// The request map is `{protocol, baseUrl, apiKey, model, payload, headers,
  /// timeoutMs, idleTimeoutMs, allowInsecureHttp, stream}` in camelCase.
  Map<String, dynamic> prepareNative(Map<String, dynamic> request) =>
      _call1(_nativePrepare, 'lingxi_native_prepare', request);

  /// Same as [prepareNative] for LMP requests; the result also carries
  /// `protocol`.
  Map<String, dynamic> prepareUnified(Map<String, dynamic> request) =>
      _call1(_unifiedPrepare, 'lingxi_unified_prepare', request);

  /// Normalized embeddings (`{embeddings, usage, raw}`).
  Map<String, dynamic> embed(Map<String, dynamic> request) {
    final envelope = _call1(_embed, 'lingxi_unified_embed', request);
    return _asMap(envelope['response'], 'embed response');
  }

  /// Normalized reranking (`{results, raw}`).
  Map<String, dynamic> rerank(Map<String, dynamic> request) {
    final envelope = _call1(_rerank, 'lingxi_unified_rerank', request);
    return _asMap(envelope['response'], 'rerank response');
  }

  // ---------------------------------------------------------------------------
  // Incremental streams
  // ---------------------------------------------------------------------------

  /// Opens a stream and returns its opaque handle; throws when the core could
  /// not open one.
  Pointer<Void> streamOpen(Map<String, dynamic> request) {
    final pointer = _allocCString(jsonEncode(request));
    try {
      final handle = _streamOpen.call(pointer);
      if (handle == nullptr) {
        throw CoreException('stream_open failed: ${streamOpenError()}', 'open_failed');
      }
      return handle;
    } finally {
      _freeBytes(pointer);
    }
  }

  /// Why the last [streamOpen] failed.
  String streamOpenError() {
    final envelope = _decode('lingxi_stream_open_error', _streamOpenError.call());
    return envelope['error'] as String? ?? 'unknown stream error';
  }

  /// Waits up to [timeoutMs] (0 = forever) for one event.
  ///
  /// Returns the raw envelope: `{"ok":true,"event":{...}}`,
  /// `{"ok":true,"done":true}` or `{"ok":false,"code","error"}`.
  Map<String, dynamic> streamNext(Pointer<Void> handle, int timeoutMs) =>
      _decode('lingxi_stream_next', _streamNext.call(handle, timeoutMs));

  /// Diagnostics for a live handle.
  Map<String, dynamic> streamLastError(Pointer<Void> handle) =>
      _decode('lingxi_stream_last_error', _streamLastError.call(handle));

  /// Requests cancellation; safe from any thread and on NULL.
  void streamCancel(Pointer<Void> handle) => _streamCancel.call(handle);

  /// Releases the handle; idempotent and NULL-safe.
  void streamClose(Pointer<Void> handle) => _streamClose.call(handle);

  // ---------------------------------------------------------------------------
  // Internals
  // ---------------------------------------------------------------------------

  Map<String, dynamic> _call1(
      _Call1 function, String operation, Map<String, dynamic> request) {
    final pointer = _allocCString(jsonEncode(request));
    try {
      return _requireOk(operation, _decode(operation, function.call(pointer)));
    } finally {
      _freeBytes(pointer);
    }
  }

  Map<String, dynamic> _call2(
      _Call2 function, String operation, String first, String second) {
    final a = _allocCString(first);
    final b = _allocCString(second);
    try {
      return _requireOk(operation, _decode(operation, function.call(a, b)));
    } finally {
      _freeBytes(a);
      _freeBytes(b);
    }
  }

  Map<String, dynamic> _requireOk(String operation, Map<String, dynamic> envelope) {
    if (envelope['ok'] == true) return envelope;
    final code = envelope['code'] as String?;
    final error = envelope['error'] as String? ?? 'unknown error';
    throw CoreException(
      code == null ? '$operation failed: $error' : '$operation failed [$code]: $error',
      code,
    );
  }

  Map<String, dynamic> _decode(String operation, Pointer<Uint8> result) {
    if (result == nullptr) {
      throw CoreException('$operation failed: the core returned NULL', 'null_result');
    }
    final String text;
    try {
      final length = _cStringLength(result);
      text = utf8.decode(result.asTypedList(length), allowMalformed: true);
    } finally {
      _stringFree.call(result);
    }
    final decoded = jsonDecode(text);
    if (decoded is! Map<String, dynamic>) {
      throw CoreException('$operation returned a non-object envelope', 'invalid_envelope');
    }
    return decoded;
  }

  int _cStringLength(Pointer<Uint8> pointer) {
    var length = 0;
    while (pointer[length] != 0) {
      length++;
    }
    return length;
  }

  Pointer<Uint8> _allocCString(String text) {
    final bytes = utf8.encode(text);
    final pointer = _malloc.call(bytes.length + 1);
    if (pointer == nullptr) throw StateError('malloc failed');
    final view = pointer.asTypedList(bytes.length + 1);
    view.setRange(0, bytes.length, bytes);
    view[bytes.length] = 0;
    return pointer;
  }

  void _freeBytes(Pointer<Uint8> pointer) {
    if (pointer != nullptr) _free.call(pointer);
  }

  Map<String, dynamic> _asMap(Object? value, String what) {
    if (value is Map<String, dynamic>) return value;
    throw CoreException('expected an object for $what', 'invalid_envelope');
  }
}

/// One open upstream stream owned by the Rust core.
///
/// [next] blocks for exactly one event, so callers observe tokens while the
/// provider response is still open. Unified sessions yield LMP events;
/// native sessions yield `{event, data, id}` frames. The transport and all
/// framing live in Rust.
///
/// Call [next] from a background worker (or accept that it blocks the calling
/// isolate), and always [close] the session. [close] is idempotent and
/// [cancel] is safe from any thread; a session never touches a released
/// handle.
class StreamSession {
  /// Timeout used by [next] when the caller does not pass one.
  static const int defaultTimeoutMs = 60000;

  final CoreBridge bridge;
  final bool unified;
  Pointer<Void> _handle;
  bool _closed = false;

  StreamSession._(this.bridge, this._handle, this.unified);

  /// Opens a stream. [request] carries protocol, baseUrl, apiKey, model,
  /// payload (LMP when [unified], provider-native otherwise), optional
  /// headers and an idle timeout.
  static StreamSession open(
    Map<String, dynamic> request, {
    bool unified = false,
    CoreBridge? bridge,
  }) {
    final core = bridge ?? CoreBridge.shared();
    final body = Map<String, dynamic>.of(request)..['unified'] = unified;
    return StreamSession._(core, core.streamOpen(body), unified);
  }

  bool get closed => _closed;

  /// Waits up to [timeoutMs] (0 = forever) for one event.
  ///
  /// Returns the event, or null once the stream ended cleanly. A terminal
  /// upstream failure throws [CoreException].
  Map<String, dynamic>? next([int timeoutMs = defaultTimeoutMs]) {
    if (_closed) throw StateError('stream is closed');
    final envelope = bridge.streamNext(_handle, timeoutMs);
    if (envelope['ok'] == true) {
      if (envelope['done'] == true) return null;
      final event = envelope['event'];
      if (event is Map<String, dynamic>) return event;
      throw CoreException('stream returned a non-object event', 'invalid_envelope');
    }
    final code = envelope['code'] as String?;
    final error = envelope['error'] as String? ?? 'unknown error';
    throw CoreException(
      'stream failed${code == null ? '' : ' [$code]'}: $error',
      code,
    );
  }

  /// Reads every remaining event; only use this when buffering is acceptable.
  List<Map<String, dynamic>> collect() {
    final events = <Map<String, dynamic>>[];
    while (true) {
      final event = next();
      if (event == null) break;
      events.add(event);
    }
    return events;
  }

  /// Asks a blocked reader to stop; safe from any thread.
  void cancel() {
    if (_closed) return;
    bridge.streamCancel(_handle);
  }

  /// Last failure recorded by the core, if any.
  String? lastError() {
    if (_closed) return null;
    final envelope = bridge.streamLastError(_handle);
    final error = envelope['error'] as String?;
    return error == null || error.isEmpty ? null : error;
  }

  /// Cancels and releases the handle; idempotent.
  void close() {
    if (_closed) return;
    _closed = true;
    bridge.streamClose(_handle);
    _handle = nullptr;
  }
}
