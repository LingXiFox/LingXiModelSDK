#!/usr/bin/env bash
set -euo pipefail

if [[ "${LINGXI_LIVE_TEST:-0}" != "1" ]]; then
  echo "Skipping live API verification (set LINGXI_LIVE_TEST=1 with caller-provided provider keys to enable)."
  exit 0
fi

echo "Live verification enabled. Checking caller-provided environment variables..."
if [[ -n "${OPENAI_API_KEY:-}" ]]; then
  echo "Probing OpenAI models endpoint..."
  curl -fsS -H "Authorization: Bearer ${OPENAI_API_KEY}" https://api.openai.com/v1/models >/dev/null
  echo "OpenAI models endpoint reachable."
fi
if [[ -n "${ANTHROPIC_API_KEY:-}" ]]; then
  echo "Probing Anthropic models endpoint..."
  curl -fsS -H "x-api-key: ${ANTHROPIC_API_KEY}" -H "anthropic-version: 2023-06-01" https://api.anthropic.com/v1/models >/dev/null
  echo "Anthropic models endpoint reachable."
fi
if [[ -n "${GEMINI_API_KEY:-}" ]]; then
  echo "Probing Gemini models endpoint..."
  curl -fsS -H "x-goog-api-key: ${GEMINI_API_KEY}" https://generativelanguage.googleapis.com/v1beta/models >/dev/null
  echo "Gemini models endpoint reachable."
fi
