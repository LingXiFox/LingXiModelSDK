"""Native Provider transport.

The Rust core owns every protocol detail: endpoint joining, auth headers,
SigV4 signing, request translation and stream framing. This module owns the
Python transport (urllib), redirect blocking and the response size caps.

Callers obtain a wire target from :meth:`NativeClient.prepare` and hand it to
:meth:`post_json` or :meth:`post_stream`, which keeps one code path per
protocol across all ten SDKs.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any

from .ffi_bridge import CoreError, RustCoreBridge
from .registry import validate_endpoint_url

MAX_BODY_BYTES = 32 * 1024 * 1024


class NativeError(Exception):
    """Raised when a provider request cannot be prepared or completed."""


class NativeHTTPStatusError(NativeError):
    def __init__(self, status: int, body: str) -> None:
        super().__init__(f"Upstream HTTP {status}: {body}")
        self.status = status
        self.body = body


class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str, headers: Any, newurl: str) -> None:
        raise NativeError(f"HTTP redirect {code} blocked for security")


_OPENER = urllib.request.build_opener(_NoRedirectHandler())


def parse_sse(text: str) -> list[dict[str, str | None]]:
    events: list[dict[str, str | None]] = []
    event_name: str | None = None
    data_lines: list[str] = []
    last_id: str | None = None

    def flush() -> None:
        nonlocal event_name, data_lines
        if data_lines or event_name is not None:
            events.append({"event": event_name, "data": "\n".join(data_lines), "id": last_id})
            event_name = None
            data_lines = []

    for raw in text.splitlines():
        line = raw.rstrip("\r")
        if line == "":
            flush()
            continue
        if line.startswith(":"):
            continue
        if ":" in line:
            field_name, val = line.split(":", 1)
            if val.startswith(" "):
                val = val[1:]
            if field_name == "event":
                event_name = val
            elif field_name == "data":
                data_lines.append(val)
            elif field_name == "id":
                last_id = val
    flush()
    return events


def parse_ndjson(text: str) -> list[dict[str, str | None]]:
    return [{"event": None, "data": line.strip(), "id": None} for line in text.splitlines() if line.strip()]


@dataclass
class NativeRequestConfig:
    base_url: str
    api_key: str | None = field(default=None, repr=False)
    headers: dict[str, str] = field(default_factory=dict, repr=False)
    timeout: float = 60.0
    allow_insecure_http: bool = False
    stream_idle_timeout: float = 120.0


@dataclass(frozen=True)
class WireTarget:
    """A fully prepared request: URL, headers, body and framing."""

    url: str
    headers: dict[str, str]
    body: dict[str, Any]
    framing: str

    @property
    def is_ndjson(self) -> bool:
        return self.framing == "ndjson"

    @property
    def is_event_stream(self) -> bool:
        return self.framing == "amazon_event_stream"


class NativeClient:
    """Transport for the native wire protocols."""

    def __init__(self, bridge: RustCoreBridge | None = None) -> None:
        self.bridge = bridge or RustCoreBridge()

    def prepare(
        self,
        protocol: str,
        cfg: NativeRequestConfig,
        model: str | None,
        payload: dict[str, Any],
        stream: bool = False,
    ) -> WireTarget:
        """Ask the core for the final URL, headers and body of one request."""
        request: dict[str, Any] = {
            "protocol": protocol,
            "baseUrl": cfg.base_url,
            "payload": payload,
            "headers": cfg.headers,
            "timeoutMs": int(cfg.timeout * 1000),
            "idleTimeoutMs": int(cfg.stream_idle_timeout * 1000),
            "allowInsecureHttp": cfg.allow_insecure_http,
            "stream": stream,
        }
        if cfg.api_key:
            request["apiKey"] = cfg.api_key
        if model:
            request["model"] = model
        try:
            prepared = self.bridge.prepare_native(request)
        except CoreError as e:
            raise NativeError(str(e)) from e
        return WireTarget(
            url=prepared["url"],
            headers=dict(prepared.get("headers") or {}),
            body=prepared.get("body") or {},
            framing=prepared.get("framing", "sse"),
        )

    def post_json(
        self,
        url: str,
        headers: dict[str, str],
        payload: dict[str, Any],
        timeout: float = 60.0,
        allow_insecure_http: bool = False,
    ) -> dict[str, Any]:
        validate_endpoint_url(url, allow_insecure_http=allow_insecure_http)
        data = json.dumps(payload).encode("utf-8")
        req_headers = {"Content-Type": "application/json", "Accept": "application/json", **headers}
        req = urllib.request.Request(url, data=data, headers=req_headers, method="POST")
        try:
            with _OPENER.open(req, timeout=timeout) as resp:
                raw = resp.read(MAX_BODY_BYTES + 1)
                if len(raw) > MAX_BODY_BYTES:
                    raise NativeError("Response exceeded 32 MiB limit")
                return json.loads(raw.decode("utf-8"))
        except urllib.error.HTTPError as e:
            raise NativeHTTPStatusError(e.code, e.read(1024 * 1024).decode("utf-8", errors="replace")) from e

    def post_stream(
        self,
        url: str,
        headers: dict[str, str],
        payload: dict[str, Any],
        ndjson: bool = False,
        timeout: float = 60.0,
        allow_insecure_http: bool = False,
    ) -> list[dict[str, str | None]]:
        """Buffered stream read.

        Kept for hosts that cannot consume events incrementally; the incremental
        path is `ffi_bridge.StreamHandle`, which applies real backpressure.
        """
        validate_endpoint_url(url, allow_insecure_http=allow_insecure_http)
        data = json.dumps(payload).encode("utf-8")
        accept = "application/x-ndjson, application/json" if ndjson else "text/event-stream"
        req_headers = {"Content-Type": "application/json", "Accept": accept, **headers}
        req = urllib.request.Request(url, data=data, headers=req_headers, method="POST")
        try:
            with _OPENER.open(req, timeout=timeout) as resp:
                raw = resp.read(MAX_BODY_BYTES + 1)
                if len(raw) > MAX_BODY_BYTES:
                    raise NativeError("Stream exceeded 32 MiB limit")
                text = raw.decode("utf-8", errors="replace")
                return parse_ndjson(text) if ndjson else parse_sse(text)
        except urllib.error.HTTPError as e:
            raise NativeHTTPStatusError(e.code, e.read(1024 * 1024).decode("utf-8", errors="replace")) from e
