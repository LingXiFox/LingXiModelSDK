#!/usr/bin/env python3
"""
Standalone protocol mock server serving all 4 Tier-1 native protocols
from conformance/native/ fixtures (zero external dependencies).
"""

from __future__ import annotations

import argparse
import http.server
import json
from pathlib import Path

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
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length > 0 else b"{}"
        try:
            body = json.loads(raw.decode("utf-8"))
        except Exception:
            body = {}
        stream = bool(body.get("stream")) or "alt=sse" in self.path

        if self.path.endswith("/chat/completions"):
            fname = "openai_chat_stream.sse" if stream else "openai_chat_response.json"
        elif self.path.endswith("/responses"):
            fname = "openai_responses_stream.sse" if stream else "openai_responses_response.json"
        elif self.path.endswith("/messages"):
            fname = "anthropic_messages_stream.sse" if stream else "anthropic_messages_response.json"
        elif ":streamGenerateContent" in self.path or ":generateContent" in self.path:
            fname = "google_gemini_stream.sse" if stream else "google_gemini_response.json"
        else:
            self._send(404, "application/json", b'{"error":"unknown_endpoint"}')
            return

        ct = "text/event-stream" if stream else "application/json"
        self._send(200, ct, (FIXTURES / fname).read_bytes())


def main() -> int:
    parser = argparse.ArgumentParser(description="LingXi standalone protocol mock server")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18099)
    args = parser.parse_args()

    srv = http.server.ThreadingHTTPServer((args.host, args.port), MockHandler)
    print(f"LingXi mock server listening on http://{args.host}:{args.port}", flush=True)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        srv.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
