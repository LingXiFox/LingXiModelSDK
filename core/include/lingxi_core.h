#ifndef LINGXI_CORE_H
#define LINGXI_CORE_H

#ifdef __cplusplus
extern "C" {
#endif

/* Translates an LMP JSON request into a provider-native JSON payload envelope. */
char *lingxi_lmp_translate_request(const char *protocol_utf8, const char *lmp_request_json_utf8);

/* Normalizes a provider-native JSON response into an LMP response JSON envelope. */
char *lingxi_lmp_normalize_response(const char *protocol_utf8, const char *native_response_json_utf8);

/* Frees a string allocated by lingxi_core. Safe on NULL. */
void lingxi_string_free(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
