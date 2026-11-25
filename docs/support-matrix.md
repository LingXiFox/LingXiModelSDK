# Provider & Protocol Support Matrix

Generated from `registry/registry.json` and `conformance/evidence/support-matrix.json`.

## Summary

- Total providers in registry: **327**
- `full_support`: **0**
- `live_verified`: **0**
- `mock_tested`: **9**
- `compatible` (declarative OpenAI-compatible, awaiting mock/live verification): **9**
- `discovered` (catalog metadata only, zero inferred protocol claims): **309**

## Verified Providers (`mock_tested`)

| Provider | Status | Protocols | Evidence |
| --- | --- | --- | --- |
| `amazon-bedrock` | `mock_tested` | `aws_bedrock` | `core/tests/agent_scenarios.rs` (2026-11-25), `core/tests/stream_incremental.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25), `sdk/typescript/tests/agent_conformance.test.ts` (2026-11-25) |
| `anthropic` | `mock_tested` | `anthropic_messages` | `core/tests/native_protocols.rs` (2026-11-25), `core/tests/unified_lmp.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25), `sdk/typescript/tests/agent_conformance.test.ts` (2026-11-25) |
| `azure` | `mock_tested` | `azure_openai` | `core/tests/agent_scenarios.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25) |
| `cohere` | `mock_tested` | `cohere` | `core/tests/agent_scenarios.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25), `sdk/typescript/tests/agent_conformance.test.ts` (2026-11-25) |
| `deepseek` | `mock_tested` | `openai_chat_completions` | `core/tests/native_protocols.rs` (2026-11-25), `sdk/go/native/native_test.go` (2026-11-24), `conformance/agent/expectations.json` (2026-11-25) |
| `google` | `mock_tested` | `google_gemini` | `core/tests/native_protocols.rs` (2026-11-25), `core/tests/unified_lmp.rs` (2026-11-25), `conformance/agent/gemini-tool-loop.json` (2026-11-24), `conformance/agent/expectations.json` (2026-11-25), `sdk/typescript/tests/browser_check.mjs` (2026-11-25) |
| `google-vertex` | `mock_tested` | `google_vertex_ai` | `core/tests/agent_scenarios.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25) |
| `ollama` | `mock_tested` | `ollama` | `core/tests/agent_scenarios.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25) |
| `openai` | `mock_tested` | `openai_responses`, `openai_chat_completions` | `core/tests/native_protocols.rs` (2026-11-25), `core/tests/unified_lmp.rs` (2026-11-25), `core/tests/agent_scenarios.rs` (2026-11-25), `conformance/agent/expectations.json` (2026-11-25), `sdk/go/unified/agent_conformance_test.go` (2026-11-25), `sdk/typescript/tests/agent_conformance.test.ts` (2026-11-25), `gateway/gateway_test.go` (2026-11-24) |

## Declarative Compatible Providers (`compatible`)

`fireworks-ai`, `groq`, `mistral`, `moonshotai`, `openrouter`, `siliconflow`, `togetherai`, `xai`, `zhipuai`
