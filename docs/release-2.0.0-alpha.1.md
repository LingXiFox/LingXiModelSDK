# LingXiModelSDK `2.0.0-alpha.1` Release & Production Cutover Guide

## 1. Release Summary (`2.0.0-alpha.1`)
- **Three Parallel Entry Points**: Model Catalog (zero inference runtime), Native Provider, and Unified Provider (LMP) across **10 languages** (Swift, Rust, Go, TypeScript, Python, Java, C++, C#, Kotlin, Dart).
- **Nine Protocols**: OpenAI Chat Completions, OpenAI Responses, Anthropic Messages, Google Gemini, AWS Bedrock (SigV4), Google Vertex AI, Azure OpenAI, Ollama, Cohere (including `/v2/rerank`).
- **327-Provider Registry**: 9 `mock_tested`, 9 `compatible`, 309 `discovered` (zero npm-based protocol inference).
- **Optional HTTP Gateway**: Real-time SSE flushing, dial-time private-IP SSRF protection, redirect blocking, fail-closed remote bind policy, `/v1/lmp/embed` & `/v1/lmp/rerank`.
- **Official Website (`models.lingxifox.cn`)**: Single `CGO_ENABLED=0` Go binary with `/`, `/sdk`, `/protocols`, `/providers`, `/gateway`, `/models.json`, `/publication.json`, `/healthz`, preserving ICP and MPS filing metadata.

## 2. Production Website Cutover & Rollback Plan (Requires Explicit Approval)
1. **Stage Binary**: Build `CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o lingxi-models .` (or download from `release-artifacts.yml`).
2. **Pre-flight Check**: Verify SHA-256 checksum, run on staging port (`127.0.0.1:18080`), check `/healthz`, `/models.json`, `/publication.json`, `/gateway`, and `/static/beian-icon.png`.
3. **Cutover**: Keep existing `/etc/caddy/Caddyfile` untouched or update only after explicit sign-off; swap systemd `lingxi-models.service` binary atomically (`install -m 0755`), keeping previous binary as `/usr/local/bin/lingxi-models.prev`.
4. **Rollback**:
   ```sh
   sudo mv /usr/local/bin/lingxi-models.prev /usr/local/bin/lingxi-models
   sudo systemctl restart lingxi-models
   ```
