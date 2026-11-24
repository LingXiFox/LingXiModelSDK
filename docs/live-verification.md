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
