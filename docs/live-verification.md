# Live completion verification

Run `./scripts/live-verify.sh` with environment variables set for the protocols you intend to test. The script runs an actual completion through the Rust Unified SDK, using a fixed short prompt. Each probe requires the selected model and all listed settings. It prints one status per protocol, returns a nonzero exit status if any configured probe fails, and keeps prompts, outputs, credentials, and upstream error bodies out of its report.

| Protocol | Model variable | Other settings |
| --- | --- | --- |
| OpenAI Chat Completions | `OPENAI_CHAT_MODEL` | `OPENAI_API_KEY` |
| OpenAI Responses | `OPENAI_RESPONSES_MODEL` | `OPENAI_API_KEY` |
| Anthropic Messages | `ANTHROPIC_MODEL` | `ANTHROPIC_API_KEY` |
| Gemini Developer API | `GEMINI_MODEL` | `GEMINI_API_KEY` |
| AWS Bedrock | `BEDROCK_MODEL` | `AWS_REGION` and `AWS_BEARER_TOKEN_BEDROCK`, or `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and optional `AWS_SESSION_TOKEN` |
| Google Vertex AI | `VERTEX_MODEL` | `GOOGLE_CLOUD_PROJECT`, `GOOGLE_CLOUD_LOCATION`, `GOOGLE_VERTEX_ACCESS_TOKEN` |
| Azure OpenAI | `AZURE_OPENAI_DEPLOYMENT` | `AZURE_OPENAI_ENDPOINT`, `AZURE_OPENAI_API_KEY`. The Unified client uses API version `2024-10-21`. |
| Ollama | `OLLAMA_MODEL` | `OLLAMA_HOST`, such as the address of a local Ollama instance |
| Cohere | `COHERE_MODEL` | `COHERE_API_KEY` |

Vertex API callers using a Native or Unified SDK supply a base URL ending in `/v1/projects/PROJECT/locations/LOCATION/publishers/google/models`. Use the regional API host, or `aiplatform.googleapis.com` for the global location. Azure callers supply the resource origin as the base URL and the deployment name as the model.

`PASS` records a successful text completion on this SDK and model. `SKIP` leaves the existing verification level intact. Record the SDK commit, protocol, model, date, and observed result when updating evidence. Record streaming, tools, embeddings and other capabilities separately after exercising them.

## Phase 8 execution record

On 2026-11-25 the probe was executed against commit `c717c3e` on the maintainer host. No provider credential was present in the environment and no local Ollama daemon was reachable, so every probe skipped:

```text
[SKIP] openai_chat_completions (endpoint configuration or OPENAI_CHAT_MODEL missing)
[SKIP] openai_responses (endpoint configuration or OPENAI_RESPONSES_MODEL missing)
[SKIP] anthropic_messages (endpoint configuration or ANTHROPIC_MODEL missing)
[SKIP] google_gemini (endpoint configuration or GEMINI_MODEL missing)
[SKIP] aws_bedrock (endpoint configuration or BEDROCK_MODEL missing)
[SKIP] google_vertex_ai (endpoint configuration or VERTEX_MODEL missing)
[SKIP] azure_openai (endpoint configuration or AZURE_OPENAI_DEPLOYMENT missing)
[SKIP] ollama (endpoint configuration or OLLAMA_MODEL missing)
[SKIP] cohere (endpoint configuration or COHERE_MODEL missing)
Completion probes: 0 passed, 9 skipped, 0 failed
```

Consequences for the support matrix: no provider gains `live_verified` or `full_support`, and `mock_tested` entries stay where they are. The skipped run is not evidence of upstream compatibility, only that the probe wiring reaches every protocol. Supply the variables above and re-run to replace this record with dated `PASS` lines.
