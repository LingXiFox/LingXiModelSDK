"""Unified LingXi Model Protocol (LMP) client backed by Rust core FFI when available."""

from __future__ import annotations

from typing import Any

from .ffi_bridge import RustCoreBridge
from .native import NativeClient, NativeRequestConfig


class UnsupportedFeatureError(Exception):
    def __init__(self, protocol: str, feature: str) -> None:
        super().__init__(f"Unsupported feature {feature!r} for protocol {protocol!r}")
        self.protocol = protocol
        self.feature = feature


class UnifiedClient:
    def __init__(self, native: NativeClient | None = None) -> None:
        self.native = native or NativeClient()
        self.bridge = RustCoreBridge()

    def complete(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        req: dict[str, Any],
    ) -> dict[str, Any]:
        if not self.bridge.available:
            raise RuntimeError("lingxi_core shared library not found; build core/ first")
        try:
            payload = self.bridge.translate_request(protocol, req)
        except RuntimeError as e:
            if "unsupported" in str(e).lower():
                raise UnsupportedFeatureError(protocol, str(e)) from e
            raise
        cfg = NativeRequestConfig(base_url=base_url, api_key=api_key)
        url, hdrs, _ = self.native.build_target(protocol, cfg, req["model"], payload)
        raw = self.native.post_json(url, hdrs, payload)
        return self.bridge.normalize_response(protocol, raw)

    def stream(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        req: dict[str, Any],
    ) -> list[dict[str, Any]]:
        if not self.bridge.available:
            raise RuntimeError("lingxi_core shared library not found; build core/ first")
        if protocol == "aws_bedrock":
            return self.bridge.stream_through_core({
                "protocol": protocol, "baseUrl": base_url, "apiKey": api_key,
                "payload": req, "unified": True, "stream": True,
            })
        payload = self.bridge.translate_request(protocol, req)
        cfg = NativeRequestConfig(base_url=base_url, api_key=api_key)
        url, hdrs, is_ndjson = self.native.build_target(protocol, cfg, req["model"], payload)
        if protocol in {"google_gemini", "google_vertex_ai"}:
            url = url.replace(":generateContent", ":streamGenerateContent?alt=sse")
        else:
            payload = {**payload, "stream": True}
        events = self.native.post_stream(url, hdrs, payload, ndjson=is_ndjson)
        return self.bridge.normalize_stream_events(protocol, events)

    def embed(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        model: str,
        inputs: list[str],
    ) -> dict[str, Any]:
        base = base_url.rstrip("/")
        if protocol == "openai_chat_completions":
            raw = self.native.post_json(
                f"{base}/embeddings",
                {"Authorization": f"Bearer {api_key}"},
                {"model": model, "input": inputs},
            )
            return {"embeddings": [d["embedding"] for d in raw.get("data", [])], "raw": raw}
        if protocol == "ollama":
            raw = self.native.post_json(f"{base}/api/embed", {}, {"model": model, "input": inputs})
            return {"embeddings": raw.get("embeddings", []), "raw": raw}
        raise UnsupportedFeatureError(protocol, "embed")

    def rerank(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        model: str,
        query: str,
        documents: list[str],
        top_n: int | None = None,
    ) -> dict[str, Any]:
        if protocol != "cohere":
            raise UnsupportedFeatureError(protocol, "rerank")
        payload: dict[str, Any] = {"model": model, "query": query, "documents": documents}
        if top_n is not None:
            payload["top_n"] = top_n
        raw = self.native.post_json(
            f"{base_url.rstrip('/')}/v2/rerank",
            {"Authorization": f"Bearer {api_key}"},
            payload,
        )
        return {"results": raw.get("results", []), "raw": raw}
