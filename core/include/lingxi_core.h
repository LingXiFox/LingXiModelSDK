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

/* Frees a string allocated by lingxi_core. Safe on NULL. */
void lingxi_string_free(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
