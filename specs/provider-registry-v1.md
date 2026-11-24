# Provider Registry Specification (v1.0)

Status: Frozen for Phase 3.

## 1. Purpose & Separation from Model Catalog

- `models.json` (Catalog) describes what models exist (`Discovered`).
- `registry/registry.json` (Provider Registry) declares how a runtime communicates with a provider: official endpoints, supported protocols, authentication scheme, default headers, thinking parameter shapes, model-level overrides, and evidence-backed verification status.
- **Strict rule**: Never infer protocol compatibility from upstream `npm` package names or catalog presence.

## 2. Entry Schema (`schemas/provider-registry-v1.schema.json`)

Each provider entry contains:
- `id` & `name`
- `officialEndpoints`: map of `protocol -> baseURL`
- `protocols`: supported protocol IDs (`openai_chat_completions`, `openai_responses`, `anthropic_messages`, `google_gemini`, etc.)
- `auth`: `{ "type": "bearer" | "x-api-key" | "x-goog-api-key" | "query_param" | "custom_header" | "api-key" | "aws_sigv4" | "none" | "unknown", "headerName"?, "prefix"?, "queryParam"?, "env": [...] }`
- `defaultHeaders` & `extraParams`
- `thinking`: `{ "mode": "none" | "effort" | "budget_tokens" | "gemini_thinking_config", "effortLevels"?, "minBudgetTokens"?, "maxBudgetTokens"? }`
- `modelOverrides`: per-model overrides for `endpoint`, `protocols`, `defaultHeaders`, `extraParams`, `thinking`
- `verificationStatus`: `"discovered"` | `"compatible"` | `"mock_tested"` | `"live_verified"` | `"full_support"`
- `verificationEvidence`: list of `{ "protocol", "level", "suite", "verifiedAt", "notes" }`
- `discoveryHints`: non-authoritative upstream `api` / `npm` hints for human triage only

## 3. Security Guardrails

- Remote registry updates MUST NOT silently redirect caller credentials to arbitrary domains.
- Endpoint overrides must be explicitly supplied by the caller; non-HTTPS endpoints are rejected unless loopback (`127.0.0.1`, `localhost`, `[::1]`) or explicitly opted-in for testing.

Discovered providers record environment variable names as metadata with `auth.type = "unknown"`. Azure and Vertex require an explicit deployment or project endpoint. See [live verification](../docs/live-verification.md) for their base URL formats.
