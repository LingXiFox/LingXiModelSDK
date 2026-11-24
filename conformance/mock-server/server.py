#!/usr/bin/env python3
"""
Standalone protocol mock server serving nine native protocols
from conformance/native/ fixtures (zero external dependencies).
"""

from __future__ import annotations

import argparse
import http.server
import json
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

FIXTURES = Path(__file__).resolve().parents[1] / "native"


class MockHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:
        return

    def _send(self, status: int, content_type: str, body: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if self.path == "/healthz":
            self._send(200, "application/json", b'{"status":"ok"}\n')
            return
        self._send(404, "application/json", b'{"error":"not_found"}')

    def do_POST(self) -> None:
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 <= length <= 16 * 1024 * 1024:
                self._send(413, "application/json", b'{"error":"body_limit"}')
                return
            body = json.loads(self.rfile.read(length) if length else b"{}")
            if not isinstance(body, dict):
                raise ValueError("expected object")
        except ValueError:
            self._send(400, "application/json", b'{"error":"invalid_request"}')
            return

        url = urlsplit(self.path)
        path = url.path
        stream = body.get("stream") is True or parse_qs(url.query).get("alt") == ["sse"]
        if path.endswith("/converse-stream"):
            fname = "aws_bedrock_stream.eventstream"
        elif path.endswith("/converse"):
            fname = "aws_bedrock_response.json"
        elif path.endswith("/chat/completions"):
            protocol = "azure_openai" if path.startswith("/openai/deployments/") else "openai_chat"
            fname = f"{protocol}_stream.sse" if stream else f"{protocol}_response.json"
        elif path.endswith("/responses"):
            fname = "openai_responses_stream.sse" if stream else "openai_responses_response.json"
        elif path.endswith("/messages"):
            fname = "anthropic_messages_stream.sse" if stream else "anthropic_messages_response.json"
        elif path.endswith((":streamGenerateContent", ":generateContent")):
            protocol = "google_vertex" if path.startswith("/v1/") else "google_gemini"
            fname = f"{protocol}_stream.sse" if stream else f"{protocol}_response.json"
        elif path == "/api/chat":
            fname = "ollama_chat_stream.ndjson" if stream else "ollama_chat_response.json"
        elif path == "/v2/chat":
            fname = "cohere_chat_stream.sse" if stream else "cohere_chat_response.json"
        elif path == "/v2/rerank":
            fname = "cohere_rerank_response.json"
        elif path.endswith("/embeddings"):
            fname = "openai_embed_response.json"
        else:
            self._send(404, "application/json", b'{"error":"unknown_endpoint"}')
            return

        content_type = {
            ".sse": "text/event-stream",
            ".ndjson": "application/x-ndjson",
            ".eventstream": "application/vnd.amazon.eventstream",
            ".json": "application/json",
        }[Path(fname).suffix]
        self._send(200, content_type, (FIXTURES / fname).read_bytes())


def main() -> int:
    parser = argparse.ArgumentParser(description="LingXi standalone protocol mock server")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18099)
    args = parser.parse_args()

    srv = http.server.ThreadingHTTPServer((args.host, args.port), MockHandler)
    print(f"LingXi mock server listening on http://{args.host}:{srv.server_port}", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
