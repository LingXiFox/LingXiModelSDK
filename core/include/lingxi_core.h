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


/* ------------------- Native wire target preparation -------------------
 *
 * lingxi_native_prepare resolves the final upstream URL, auth headers, request
 * body and wire framing for one request, so foreign SDKs never re-implement
 * endpoint joining, protocol auth headers or AWS SigV4. It performs no I/O and
 * is safe to call from any thread, including inside a host async loop.
 *
 * Request (camelCase; payload is the provider-native JSON body):
 *   {"protocol":"anthropic_messages","baseUrl":"https://api.anthropic.com",
 *    "apiKey":"...","model":"claude-sonnet-4","payload":{...},
 *    "headers":{},"timeoutMs":60000,"idleTimeoutMs":120000,
 *    "allowInsecureHttp":false,"stream":false}
 *
 * Response:
 *   {"ok":true,"url":"...","headers":{"x-api-key":"..."},
 *    "body":{...},"framing":"sse"}
 *   framing is "sse", "ndjson" or "amazon_event_stream".
 *   {"ok":false,"code":"registry|invalid_endpoint|auth_missing|unsupported_protocol|invalid_request","error":"..."}
 */
char *lingxi_native_prepare(const char *request_json_utf8);

/* Same contract as lingxi_native_prepare, but the input is an LMP request that
 * the core translates first. This is the shortest path for a Unified client:
 * one call yields the provider-native body plus the URL and headers to send it
 * to, and lingxi_lmp_normalize_response / the incremental normalizer turn the
 * answer back into LMP.
 *
 * Request:
 *   {"protocol":"anthropic_messages","baseUrl":"https://api.anthropic.com",
 *    "apiKey":"...","model":"claude-sonnet-4","request":{<LMP request>},
 *    "headers":{},"timeoutMs":60000,"idleTimeoutMs":120000,
 *    "allowInsecureHttp":false,"stream":false}
 *
 * Response adds the resolved protocol:
 *   {"ok":true,"protocol":"anthropic_messages","url":"...","headers":{},
 *    "body":{...},"framing":"sse"}
 *   {"ok":false,"code":"unsupported_feature|auth_missing|invalid_endpoint|...","error":"..."}
 */
char *lingxi_unified_prepare(const char *request_json_utf8);

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

/* ---------------- Unified embeddings and reranking ---------------- */

/* {"ok":true,"response":{embeddings,usage,raw}} or {"ok":false,"code","error"}.
 * Request: {"protocol","baseUrl","apiKey","model","input":[...]} */
char *lingxi_unified_embed(const char *request_json_utf8);

/* {"ok":true,"response":{results:[{index,relevanceScore}],raw}}.
 * Request: {"protocol","baseUrl","apiKey","model","query","documents":[...],"topN"} */
char *lingxi_unified_rerank(const char *request_json_utf8);

/* --------------------- Incremental LMP normalization ---------------------
 *
 * lingxi_lmp_normalize_stream_events needs the complete event array, so a
 * caller that owns the HTTP transport (URLSession, HttpClient, HttpClient,
 * dart:io, ...) would have to buffer the whole upstream response before it
 * sees any LMP event. These sessions instead normalize one frame at a time:
 *
 *   LingxiNormalizer *n = lingxi_lmp_normalizer_open("anthropic_messages");
 *   char *env = lingxi_lmp_normalizer_push(n, "{\"data\":\"{\\\"type\\\":...}\"}");
 *   // env = {"ok":true,"events":[{"type":"text_delta",...}],"done":false}
 *   char *tail = lingxi_lmp_normalizer_finish(n);
 *   lingxi_lmp_normalizer_close(n);
 *
 * Sessions hold protocol state only (tool-call argument accumulation, thinking
 * signatures, usage merge) and perform no I/O, so they are safe to call from
 * the host async loop.
 */
typedef struct LingxiNormalizer LingxiNormalizer;

/* NULL on unknown protocol; reason via lingxi_normalizer_open_error(). */
LingxiNormalizer *lingxi_lmp_normalizer_open(const char *protocol_utf8);

/* JSON error envelope for the most recent NULL open. */
char *lingxi_normalizer_open_error(void);

/* Feeds one frame; returns the LMP events it produced. */
char *lingxi_lmp_normalizer_push(LingxiNormalizer *normalizer, const char *event_json_utf8);

/* Emits trailing state such as a final message_end. */
char *lingxi_lmp_normalizer_finish(LingxiNormalizer *normalizer);

/* Releases the session. Idempotent and NULL-safe. */
void lingxi_lmp_normalizer_close(LingxiNormalizer *normalizer);

/* Frees a string allocated by lingxi_core. Safe on NULL. */
void lingxi_string_free(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
