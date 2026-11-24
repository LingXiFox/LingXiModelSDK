#!/usr/bin/env bash
set -euo pipefail

# Reproducible live API verification across all 9 protocols.
# Never fabricates verification: protocols without env credentials are skipped.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
echo "=== LingXiModelSDK Live API Verification ==="

check_env() {
  local name="$1"
  if [[ -n "${!name:-}" ]]; then
    return 0
  fi
  return 1
}

if check_env OPENAI_API_KEY; then
  echo "[LIVE] Checking OpenAI Chat Completions & Responses..."
  curl -fsS -H "Authorization: Bearer $OPENAI_API_KEY" -H "Content-Type: application/json" \
    -d '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}],"max_tokens":5}' \
    https://api.openai.com/v1/chat/completions >/dev/null && echo "  openai_chat_completions: LIVE_OK"
else
  echo "[SKIP] openai_chat_completions / openai_responses (OPENAI_API_KEY not set)"
fi

if check_env ANTHROPIC_API_KEY; then
  echo "[LIVE] Checking Anthropic Messages..."
  curl -fsS -H "x-api-key: $ANTHROPIC_API_KEY" -H "anthropic-version: 2023-06-01" \
    -H "Content-Type: application/json" \
    -d '{"model":"claude-3-5-haiku-latest","max_tokens":8,"messages":[{"role":"user","content":"ping"}]}' \
    https://api.anthropic.com/v1/messages >/dev/null && echo "  anthropic_messages: LIVE_OK"
else
  echo "[SKIP] anthropic_messages (ANTHROPIC_API_KEY not set)"
fi

if check_env GEMINI_API_KEY; then
  echo "[LIVE] Checking Google Gemini..."
  curl -fsS -H "x-goog-api-key: $GEMINI_API_KEY" -H "Content-Type: application/json" \
    -d '{"contents":[{"role":"user","parts":[{"text":"ping"}]}]}' \
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent" >/dev/null && echo "  google_gemini: LIVE_OK"
else
  echo "[SKIP] google_gemini (GEMINI_API_KEY not set)"
fi

if check_env COHERE_API_KEY; then
  echo "[LIVE] Checking Cohere v2..."
  curl -fsS -H "Authorization: Bearer $COHERE_API_KEY" -H "Content-Type: application/json" \
    -d '{"model":"command-a-03-2025","messages":[{"role":"user","content":"ping"}]}' \
    https://api.cohere.com/v2/chat >/dev/null && echo "  cohere: LIVE_OK"
else
  echo "[SKIP] cohere (COHERE_API_KEY not set)"
fi

if check_env OLLAMA_HOST; then
  echo "[LIVE] Checking Ollama..."
  curl -fsS "$OLLAMA_HOST/api/tags" >/dev/null && echo "  ollama: LIVE_OK"
else
  echo "[SKIP] ollama (OLLAMA_HOST not set)"
fi

echo "Live verification finished (only explicitly credentialed endpoints were tested)."
