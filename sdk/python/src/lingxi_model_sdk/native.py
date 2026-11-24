"""Native Provider communication using Python stdlib + AWS SigV4 + redirect blocking."""

from __future__ import annotations

import hashlib
import hmac
import json
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any
from urllib.parse import urlparse

from .registry import validate_endpoint_url


class NativeError(Exception):
    pass


class NativeHTTPStatusError(NativeError):
    def __init__(self, status: int, body: str) -> None:
        super().__init__(f"Upstream HTTP {status}: {body}")
        self.status = status
        self.body = body


class _NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req: Any, fp: Any, code: int, msg: str, headers: Any, newurl: str) -> None:
        raise NativeError(f"HTTP redirect {code} blocked for security")


_OPENER = urllib.request.build_opener(_NoRedirectHandler())


def sign_sigv4(
    method: str,
    canonical_uri: str,
    host: str,
    region: str,
    service: str,
    access_key: str,
    secret_key: str,
    payload: bytes,
    session_token: str | None = None,
) -> dict[str, str]:
    amz_date = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    date_stamp = amz_date[:8]
    payload_hash = hashlib.sha256(payload).hexdigest()
    hdrs = {"host": host, "x-amz-date": amz_date}
    if session_token:
        hdrs["x-amz-security-token"] = session_token
    keys = sorted(hdrs.keys())
    canonical_headers = "".join(f"{k}:{hdrs[k].strip()}\n" for k in keys)
    signed_headers = ";".join(keys)
    canonical_req = f"{method}\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    scope = f"{date_stamp}/{region}/{service}/aws4_request"
    sts = f"AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{hashlib.sha256(canonical_req.encode()).hexdigest()}"

    def _hmac(k: bytes, m: str) -> bytes:
        return hmac.new(k, m.encode("utf-8"), hashlib.sha256).digest()

    k_date = _hmac(f"AWS4{secret_key}".encode("utf-8"), date_stamp)
    k_region = _hmac(k_date, region)
    k_service = _hmac(k_region, service)
    k_signing = _hmac(k_service, "aws4_request")
    sig = hmac.new(k_signing, sts.encode("utf-8"), hashlib.sha256).hexdigest()
    out = {
        "Authorization": f"AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={sig}",
        "X-Amz-Date": amz_date,
    }
    if session_token:
        out["X-Amz-Security-Token"] = session_token
    return out


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
    api_key: str | None = None
    headers: dict[str, str] = field(default_factory=dict)
    timeout: float = 60.0
    allow_insecure_http: bool = False


class NativeClient:
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
                raw = resp.read(32 * 1024 * 1024 + 1)
                if len(raw) > 32 * 1024 * 1024:
                    raise NativeError("Response exceeded 32 MiB limit")
                return json.loads(raw.decode("utf-8"))
        except urllib.error.HTTPError as e:
            raise NativeHTTPStatusError(e.code, e.read().decode("utf-8", errors="replace")) from e

    def post_stream(
        self,
        url: str,
        headers: dict[str, str],
        payload: dict[str, Any],
        ndjson: bool = False,
        timeout: float = 60.0,
        allow_insecure_http: bool = False,
    ) -> list[dict[str, str | None]]:
        validate_endpoint_url(url, allow_insecure_http=allow_insecure_http)
        data = json.dumps(payload).encode("utf-8")
        accept = "application/x-ndjson, application/json" if ndjson else "text/event-stream"
        req_headers = {"Content-Type": "application/json", "Accept": accept, **headers}
        req = urllib.request.Request(url, data=data, headers=req_headers, method="POST")
        try:
            with _OPENER.open(req, timeout=timeout) as resp:
                text = resp.read(32 * 1024 * 1024).decode("utf-8", errors="replace")
                return parse_ndjson(text) if ndjson else parse_sse(text)
        except urllib.error.HTTPError as e:
            raise NativeHTTPStatusError(e.code, e.read().decode("utf-8", errors="replace")) from e

    def build_target(
        self,
        protocol: str,
        cfg: NativeRequestConfig,
        model: str,
        payload: dict[str, Any],
    ) -> tuple[str, dict[str, str], bool]:
        base = cfg.base_url.rstrip("/")
        hdrs = dict(cfg.headers)
        key = cfg.api_key or ""
        if protocol == "openai_chat_completions":
            hdrs.setdefault("Authorization", f"Bearer {key}")
            return f"{base}/chat/completions", hdrs, False
        if protocol == "openai_responses":
            hdrs.setdefault("Authorization", f"Bearer {key}")
            return f"{base}/responses", hdrs, False
        if protocol == "anthropic_messages":
            hdrs.setdefault("x-api-key", key)
            hdrs.setdefault("anthropic-version", "2023-06-01")
            url = f"{base}/messages" if base.endswith("/v1") else f"{base}/v1/messages"
            return url, hdrs, False
        if protocol == "google_gemini":
            hdrs.setdefault("x-goog-api-key", key)
            return f"{base}/v1beta/models/{model}:generateContent", hdrs, False
        if protocol == "google_vertex_ai":
            hdrs.setdefault("Authorization", f"Bearer {key}")
            return f"{base}/v1/publishers/google/models/{model}:generateContent", hdrs, False
        if protocol == "azure_openai":
            hdrs.setdefault("api-key", key)
            ver = hdrs.get("x-azure-api-version", "2024-10-21")
            return f"{base}/openai/deployments/{model}/chat/completions?api-version={ver}", hdrs, False
        if protocol == "ollama":
            return f"{base}/api/chat", hdrs, True
        if protocol == "cohere":
            hdrs.setdefault("Authorization", f"Bearer {key}")
            return f"{base}/v2/chat", hdrs, False
        if protocol == "aws_bedrock":
            uri = f"/model/{model}/converse"
            parts = key.split(":")
            if len(parts) >= 2:
                parsed = urlparse(base)
                signed = sign_sigv4(
                    method="POST",
                    canonical_uri=uri,
                    host=parsed.netloc,
                    region=hdrs.get("x-aws-region", "us-east-1"),
                    service="bedrock",
                    access_key=parts[0],
                    secret_key=parts[1],
                    session_token=parts[2] if len(parts) >= 3 else None,
                    payload=json.dumps(payload).encode("utf-8"),
                )
                hdrs.update(signed)
            else:
                hdrs.setdefault("Authorization", f"Bearer {key}")
            return f"{base}{uri}", hdrs, True
        raise NativeError(f"Unsupported protocol {protocol!r}")
