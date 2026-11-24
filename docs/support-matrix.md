# Provider & Protocol Support Matrix

Generated from `registry/registry.json` and `conformance/evidence/support-matrix.json`.

## Summary

- Total providers in registry: **326**
- `full_support`: **0**
- `live_verified`: **0**
- `mock_tested`: **4**
- `compatible` (declarative OpenAI-compatible, awaiting mock/live verification): **9**
- `discovered` (catalog metadata only, zero inferred protocol claims): **313**

## Verified Providers (`mock_tested`)

| Provider | Status | Protocols | Evidence |
| --- | --- | --- | --- |
| `anthropic` | `mock_tested` | `anthropic_messages` | `core/tests/native_protocols.rs` (2026-11-24), `core/tests/unified_lmp.rs` (2026-11-24) |
| `deepseek` | `mock_tested` | `openai_chat_completions` | `core/tests/native_protocols.rs` (2026-11-24), `sdk/go/native/native_test.go` (2026-11-24) |
| `google` | `mock_tested` | `google_gemini` | `core/tests/native_protocols.rs` (2026-11-24), `core/tests/unified_lmp.rs` (2026-11-24) |
| `openai` | `mock_tested` | `openai_responses`, `openai_chat_completions` | `core/tests/native_protocols.rs` (2026-11-24), `core/tests/unified_lmp.rs` (2026-11-24), `gateway/gateway_test.go` (2026-11-24) |

## Declarative Compatible Providers (`compatible`)

`fireworks-ai`, `groq`, `mistral`, `moonshotai`, `openrouter`, `siliconflow`, `togetherai`, `xai`, `zhipuai`
