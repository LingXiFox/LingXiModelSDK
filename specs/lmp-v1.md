# LingXi Model Protocol (LMP v1.0)

Status: Frozen for Phase 4 Unified Provider clients.

## 1. Core Principles

- **Opt-in normalization**: Unified Provider is a parallel entry point alongside Model Catalog and Native Provider. Neither Catalog nor Native depends on LMP.
- **No silent downgrade**: Unmappable capabilities (e.g., `json_schema` or `embed` on Anthropic Messages, `rerank` on providers without a rerank endpoint, `budget_tokens` on OpenAI Chat Completions) MUST return `UnsupportedFeature` explicitly.
- **Native escape hatch**: `providerExtensions` on requests and `raw` / `providerMetadata` on responses preserve upstream-specific fields.
- **Agent boundary**: The Agent owns session state, tool execution, permission approvals, context compaction, and workflow orchestration.

## 2. Unified Types

- **Messages**: `role` (`system` | `user` | `assistant` | `tool`), `content` (list of typed blocks: `text`, `image_url`, `image_base64`, `audio`, `thinking` with optional opaque `signature`, `tool_call`, `tool_result`).
- **Tools**: `name`, `description`, `parameters` (JSON Schema), `toolChoice` (`auto` | `none` | `required` | `tool`).
- **Thinking**: `effort` (`minimal` | `low` | `medium` | `high`) or `budgetTokens` (integer).
- **Structured output**: `responseFormat` (`text` | `json_object` | `json_schema` with `name`, `schema`, `strict`).
- **Usage & Prompt Cache**: `inputTokens`, `outputTokens`, `totalTokens`, `reasoningTokens`, `cacheReadTokens`, `cacheWriteTokens`.
- **Stream Events**: `message_start`, `thinking_delta`, `text_delta`, `tool_call_delta`, `usage`, `message_end`, `error`.
- **Extensions**: `EmbeddingRequest`/`EmbeddingResponse`, `RerankRequest`/`RerankResponse`.
- **Error Taxonomy**: `UnsupportedFeature`, `Auth`, `RateLimit`, `InvalidRequest`, `ProviderError`, `Timeout`, `Cancelled`, `Transport`, `Decode`.
