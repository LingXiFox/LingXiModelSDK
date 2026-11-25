"""Unified LingXi Model Protocol (LMP) client.

Request translation, endpoint joining, authentication and response/stream
normalization all happen in the Rust core through the C ABI. This module adds
Python ergonomics: dict-free streaming, typed errors and embeddings/reranking.

Every Unified call returns LMP shapes. A provider payload never leaks through,
and a capability the protocol cannot express raises
:class:`UnsupportedFeatureError` instead of silently degrading.
"""

from __future__ import annotations

from typing import Any, Iterator

from .ffi_bridge import CoreError, RustCoreBridge, StreamHandle
from .native import NativeClient, NativeRequestConfig


class UnsupportedFeatureError(Exception):
    """A capability the chosen protocol cannot express."""

    def __init__(self, protocol: str, feature: str) -> None:
        super().__init__(f"Unsupported feature {feature!r} for protocol {protocol!r}")
        self.protocol = protocol
        self.feature = feature


def _feature_name(message: str) -> str:
    """Extract the feature from the core message `unsupported feature 'x' for ...`."""
    marker = "unsupported feature '"
    start = message.find(marker)
    if start < 0:
        return "unknown"
    start += len(marker)
    end = message.find("'", start)
    return message[start:end] if end > start else "unknown"


def _raise_core(operation: str, error: CoreError, protocol: str) -> None:
    if error.code == "unsupported_feature":
        raise UnsupportedFeatureError(protocol, _feature_name(error.message)) from error
    raise error


class UnifiedStream:
    """Incremental LMP event stream with real backpressure.

    Events are pulled one at a time from the core: nothing is buffered ahead of
    the consumer, and `cancel()` interrupts a read already in flight.
    """

    def __init__(self, handle: StreamHandle, protocol: str) -> None:
        self._handle = handle
        self.protocol = protocol

    def next_event(self, timeout_ms: int = 600_000) -> dict[str, Any] | None:
        """Return the next LMP event, or None once the stream has ended."""
        try:
            return self._handle.next_event(timeout_ms)
        except CoreError as error:
            _raise_core("stream_next", error, self.protocol)
            return None

    def cancel(self) -> None:
        self._handle.cancel()

    def close(self) -> None:
        self._handle.close()

    @property
    def closed(self) -> bool:
        return self._handle.closed

    @property
    def finished(self) -> bool:
        return self._handle.finished

    def __iter__(self) -> "UnifiedStream":
        return self

    def __next__(self) -> dict[str, Any]:
        event = self.next_event()
        if event is None:
            raise StopIteration
        return event

    def __enter__(self) -> "UnifiedStream":
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()


class UnifiedClient:
    def __init__(
        self,
        native: NativeClient | None = None,
        bridge: RustCoreBridge | None = None,
    ) -> None:
        self.bridge = bridge or RustCoreBridge()
        self.native = native or NativeClient(self.bridge)

    # -- transport helpers -------------------------------------------------

    def _prepared(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        request: dict[str, Any],
        cfg: NativeRequestConfig | None = None,
        stream: bool = False,
    ) -> dict[str, Any]:
        cfg = cfg or NativeRequestConfig(base_url=base_url, api_key=api_key)
        body: dict[str, Any] = {
            "protocol": protocol,
            "baseUrl": cfg.base_url,
            "request": request,
            "headers": cfg.headers,
            "timeoutMs": int(cfg.timeout * 1000),
            "idleTimeoutMs": int(cfg.stream_idle_timeout * 1000),
            "allowInsecureHttp": cfg.allow_insecure_http,
            "stream": stream,
        }
        if cfg.api_key:
            body["apiKey"] = cfg.api_key
        model = request.get("model")
        if isinstance(model, str) and model:
            body["model"] = model
        return self.bridge.prepare_unified(body)

    # -- completions -------------------------------------------------------

    def complete(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        request: dict[str, Any],
        cfg: NativeRequestConfig | None = None,
    ) -> dict[str, Any]:
        """Run one non-streaming LMP request and return an LMP response."""
        prepared = self._prepared(protocol, base_url, api_key, request, cfg)
        raw = self.native.post_json(
            prepared["url"],
            dict(prepared.get("headers") or {}),
            prepared.get("body") or {},
            timeout=(cfg.timeout if cfg else 60.0),
            allow_insecure_http=(cfg.allow_insecure_http if cfg else False),
        )
        return self.bridge.normalize_response(protocol, raw)

    def complete_text(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        model: str,
        prompt: str,
        cfg: NativeRequestConfig | None = None,
    ) -> dict[str, Any]:
        request = {
            "model": model,
            "messages": [{"role": "user", "content": [{"type": "text", "text": prompt}]}],
        }
        return self.complete(protocol, base_url, api_key, request, cfg)

    # -- streaming ---------------------------------------------------------

    def stream(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        request: dict[str, Any],
        cfg: NativeRequestConfig | None = None,
    ) -> UnifiedStream:
        """Open an incremental LMP stream.

        Returns as soon as the upstream response headers arrive; events are
        produced as the provider writes them.
        """
        if cfg and cfg.headers:
            raise ValueError("unified streams do not accept custom HTTP headers")
        body: dict[str, Any] = {
            "protocol": protocol,
            "baseUrl": cfg.base_url if cfg else base_url,
            "payload": request,
            "unified": True,
            "stream": True,
            "idleTimeoutMs": int((cfg.stream_idle_timeout if cfg else 120.0) * 1000),
        }
        key = cfg.api_key if cfg else api_key
        if key:
            body["apiKey"] = key
        handle = self.bridge.open_stream(body)
        if handle is None:
            raise RuntimeError(f"stream_open failed: {self.bridge.stream_open_error()}")
        return UnifiedStream(handle, protocol)

    def stream_all(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        request: dict[str, Any],
        cfg: NativeRequestConfig | None = None,
    ) -> list[dict[str, Any]]:
        """Collect a stream into a list. The incremental path is :meth:`stream`."""
        events: list[dict[str, Any]] = []
        stream = self.stream(protocol, base_url, api_key, request, cfg)
        try:
            for event in stream:
                events.append(event)
        finally:
            stream.close()
        return events

    # -- embeddings and reranking ------------------------------------------

    def embed(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        model: str,
        inputs: list[str],
        cfg: NativeRequestConfig | None = None,
    ) -> dict[str, Any]:
        request: dict[str, Any] = {
            "protocol": protocol,
            "baseUrl": cfg.base_url if cfg else base_url,
            "model": model,
            "input": inputs,
        }
        if cfg.api_key if cfg else api_key:
            request["apiKey"] = cfg.api_key if cfg else api_key
        try:
            return self.bridge.embed(request)
        except CoreError as error:
            _raise_core("embed", error, protocol)
            raise

    def rerank(
        self,
        protocol: str,
        base_url: str,
        api_key: str | None,
        model: str,
        query: str,
        documents: list[str],
        top_n: int | None = None,
        cfg: NativeRequestConfig | None = None,
    ) -> dict[str, Any]:
        request: dict[str, Any] = {
            "protocol": protocol,
            "baseUrl": cfg.base_url if cfg else base_url,
            "model": model,
            "query": query,
            "documents": documents,
        }
        if cfg.api_key if cfg else api_key:
            request["apiKey"] = cfg.api_key if cfg else api_key
        if top_n is not None:
            request["topN"] = top_n
        try:
            return self.bridge.rerank(request)
        except CoreError as error:
            _raise_core("rerank", error, protocol)
            raise
