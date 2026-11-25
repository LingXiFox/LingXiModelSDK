#ifndef LINGXI_CORE_H
#define LINGXI_CORE_H

#ifdef __cplusplus
extern "C" {
#endif

/* Translates an LMP JSON request into a provider-native JSON payload envelope. */
char *lingxi_lmp_translate_request(const char *protocol_utf8, const char *lmp_request_json_utf8);

/* Normalizes a provider-native JSON response into an LMP response JSON envelope. */
char *lingxi_lmp_normalize_response(const char *protocol_utf8, const char *native_response_json_utf8);

/* Normalizes an array of SSE/NDJSON events into LMP stream events JSON envelope. */
char *lingxi_lmp_normalize_stream_events(const char *protocol_utf8, const char *events_json_utf8);

/* Resolves and validates a provider endpoint from a registry JSON document. */
char *lingxi_registry_resolve_endpoint(
    const char *registry_json_utf8,
    const char *provider_id_utf8,
    const char *protocol_utf8);

/* Blocking HTTP: protocol, baseUrl, optional apiKey/model/timeoutMs, payload,
 * optional native headers, unified (default false), stream (buffered).
 * Returns a JSON result envelope. Call off the application's async event loop. */
char *lingxi_runtime_request(const char *request_json_utf8);

/* ------------------------- Incremental streaming -------------------------
 *
 * Streaming is handle-based so an FFI caller observes provider events while
 * the upstream response is still open:
 *
 *   LingxiStream *stream = lingxi_stream_open(request_json);
 *   if (!stream) { char *err = lingxi_stream_open_error(); ... }
 *   while (1) {
 *     char *envelope = lingxi_stream_next(stream, 30000);
 *     ...  // {"ok":true,"event":...} or {"ok":true,"done":true}
 *     lingxi_string_free(envelope);
 *   }
 *   lingxi_stream_close(stream);
 *
 * `request_json` uses the lingxi_runtime_request shape (protocol, baseUrl,
 * apiKey, model, payload, headers, timeoutMs, unified) plus optional
 * `idleTimeoutMs` (default 120000) for the maximum silence tolerated between
 * events. Backpressure is consumer-driven: bytes are only read from the socket
 * when lingxi_stream_next asks for another event.
 *
 * Rules:
 *  - NULL handles are rejected by every call; closing is idempotent, so a
 *    double close or a use-after-close is a safe error, never a crash.
 *  - lingxi_stream_cancel may be called from any thread, including while
 *    lingxi_stream_next is blocked; the blocked call then returns a
 *    `cancelled` error envelope.
 *  - The open/next/close calls block and must not be invoked on the async
 *    event loop thread of the host language.
 */
typedef struct LingxiStream LingxiStream;

/* Returns NULL on failure; the reason is read from lingxi_stream_open_error(). */
LingxiStream *lingxi_stream_open(const char *request_json_utf8);

/* JSON error envelope describing why the most recent open returned NULL. */
char *lingxi_stream_open_error(void);

/* Blocks for at most timeout_ms (0 = forever) for exactly one event. */
char *lingxi_stream_next(LingxiStream *stream, unsigned long long timeout_ms);

/* Asynchronous cancellation; safe from any thread and on NULL. */
void lingxi_stream_cancel(LingxiStream *stream);

/* Releases the handle and aborts the request; idempotent and NULL-safe. */
void lingxi_stream_close(LingxiStream *stream);

/* Last error recorded for the handle, plus diagnostics (JSON envelope). */
char *lingxi_stream_last_error(LingxiStream *stream);

/* Frees a string allocated by lingxi_core. Safe on NULL. */
void lingxi_string_free(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
