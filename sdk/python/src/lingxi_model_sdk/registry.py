"""Standalone Provider Registry module for LingXiModelSDK (zero inference runtime)."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any
from urllib.parse import urlparse

ALL_PROTOCOLS: tuple[str, ...] = (
    "openai_chat_completions",
    "openai_responses",
    "anthropic_messages",
    "google_gemini",
    "aws_bedrock",
    "google_vertex_ai",
    "azure_openai",
    "ollama",
    "cohere",
)


class RegistryError(Exception):
    pass


def validate_endpoint_url(url: str, allow_insecure_http: bool = False) -> None:
    parsed = urlparse(url)
    if parsed.scheme == "https" and parsed.netloc:
        return
    if parsed.scheme == "http" and parsed.netloc:
        host = (parsed.hostname or "").lower()
        if host in {"127.0.0.1", "localhost", "::1"} or allow_insecure_http:
            return
        raise RegistryError(f"Non-HTTPS endpoint '{url}' is disallowed for non-loopback hosts")
    raise RegistryError(f"Invalid or unsupported endpoint URL '{url}'")


@dataclass(frozen=True)
class ProviderEntry:
    id: str
    name: str
    protocols: list[str]
    official_endpoints: dict[str, str]
    auth: dict[str, Any]
    default_headers: dict[str, str] = field(default_factory=dict)
    model_overrides: dict[str, Any] = field(default_factory=dict)
    verification_status: str = "discovered"


class ProviderRegistry:
    def __init__(self, providers: dict[str, ProviderEntry]) -> None:
        self._providers = providers

    @classmethod
    def from_json(cls, raw: str | bytes) -> "ProviderRegistry":
        doc = json.loads(raw)
        if doc.get("schemaVersion") != "1.0" or not isinstance(doc.get("providers"), dict):
            raise RegistryError(f"Unsupported registry schemaVersion: {doc.get('schemaVersion')!r}")
        entries: dict[str, ProviderEntry] = {}
        for pid, item in doc["providers"].items():
            entries[pid] = ProviderEntry(
                id=item["id"],
                name=item["name"],
                protocols=list(item.get("protocols", [])),
                official_endpoints=dict(item.get("officialEndpoints", {})),
                auth=dict(item.get("auth", {})),
                default_headers=dict(item.get("defaultHeaders", {})),
                model_overrides=dict(item.get("modelOverrides", {})),
                verification_status=item.get("verificationStatus", "discovered"),
            )
        return cls(entries)

    def __len__(self) -> int:
        return len(self._providers)

    def get(self, provider_id: str) -> ProviderEntry | None:
        return self._providers.get(provider_id)

    def resolve_endpoint(
        self,
        provider_id: str,
        protocol: str,
        model_id: str | None = None,
        endpoint_override: str | None = None,
        allow_insecure_http: bool = False,
    ) -> tuple[ProviderEntry, str]:
        entry = self.get(provider_id)
        if entry is None:
            raise RegistryError(f"Unknown provider '{provider_id}'")
        if protocol not in entry.protocols:
            raise RegistryError(f"Provider '{provider_id}' does not declare protocol '{protocol}'")
        model_base = None
        if model_id and model_id in entry.model_overrides:
            model_base = entry.model_overrides[model_id].get("endpoint")
        base = endpoint_override or model_base or entry.official_endpoints.get(protocol)
        if not base:
            raise RegistryError(f"No endpoint configured for {provider_id}/{protocol}")
        clean = base.rstrip("/")
        validate_endpoint_url(clean, allow_insecure_http=allow_insecure_http)
        return entry, clean
