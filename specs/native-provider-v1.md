# Native Provider Specification (v1.0)

Status: Frozen for Phase 3 Tier-1 protocols.

## 1. Architectural Invariants

- Native Provider connects directly from the caller's process to the upstream provider endpoint using the provider's native wire protocol.
- Neither Model Catalog nor Unified Provider (LMP) nor HTTP Gateway is a required dependency.
- All caller-supplied native payload fields, unknown extensions, response JSON, HTTP error bodies, and raw SSE events are preserved verbatim without silent downgrade.

## 2. Tier-1 Native Protocols

1. **`openai_chat_completions`**
   - Endpoint: `POST {base}/chat/completions`
   - Auth: `Authorization: Bearer <key>`
   - Streaming: `stream: true`, SSE `data: <json>` and `data: [DONE]`
2. **`openai_responses`**
   - Endpoint: `POST {base}/responses`
   - Auth: `Authorization: Bearer <key>`
   - Streaming: `stream: true`, typed SSE (`event: response.*`, `data: <json>`) and `[DONE]`
3. **`anthropic_messages`**
   - Endpoint: `POST {base}/v1/messages` (or `{base}/messages` if `/v1` is already in base)
   - Auth: `x-api-key: <key>` and `anthropic-version: 2023-06-01`
   - Streaming: `stream: true`, typed SSE (`event: message_start|content_block_delta|message_stop`, `data: <json>`)
4. **`google_gemini`**
   - Endpoint: `POST {base}/v1beta/models/{model}:generateContent` (non-streaming) and `POST {base}/v1beta/models/{model}:streamGenerateContent?alt=sse` (streaming)
   - Auth: `x-goog-api-key: <key>`
   - Streaming: SSE `data: <json>`
