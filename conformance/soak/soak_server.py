#!/usr/bin/env python3
"""
Loopback upstream used by `scripts/soak.sh`.

It answers every native wire path with the provider-specific fixture shape,
paced in chunks, so a soak run exercises incremental reads, connection reuse
and mid-stream cancellation instead of one large response body. Nothing here
touches the network beyond 127.0.0.1.
"""

from __future__ import annotations

import argparse
import json
import signal
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CHUNK_FRAMES = 6
CHUNK_DELAY = 0.02
SLOW_DELAY = 60.0
INPUT_TOKENS = 5
OUTPUT_TOKENS = 3


def openai_chunk(index: int) -> dict:
    return {
        "id": "soak-1",
        "object": "chat.completion.chunk",
        "model": "soak",
        "choices": [{"index": 0, "delta": {"content": f"chunk {index} "}}],
    }


def sse(payload: object) -> bytes:
    return f"data: {json.dumps(payload)}\n\n".encode("utf-8");


def sse_named(name: str, payload: object) -> bytes:
    return f"event: {name}\ndata: {json.dumps(payload)}\n\n".encode("utf-8");


def ndjson(payload: object) -> bytes:
    return (json.dumps(payload) + "\n").encode("utf-8");


def bedrock_delta(index: int) -> dict:
    return {"contentBlockDelta": {"contentBlockIndex": 0, "delta": {"text": f"chunk {index} "}}}


def gemini_frame(index: int) -> dict:
    return {
        "candidates": [
            {"content": {"parts": [{"text": f"chunk {index} "}]}, "index": 0}
        ]
    }


class SoakHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    requests_served = 0
    connections_seen: set[int] = set()
    lock = threading.Lock()

    def log_message(self, format: str, *args: object) -> None:  # noqa: A002
        return

    def count_request(self) -> None:
        with SoakHandler.lock:
            SoakHandler.requests_served += 1
            SoakHandler.connections_seen.add(id(self.connection))
            if SoakHandler.requests_served % 100 == 0:
                print(
                    json.dumps({
                        "requests": SoakHandler.requests_served,
                        "connections": len(SoakHandler.connections_seen),
                    }),
                    flush=True,
                )

    def read_body(self) -> dict:
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length) if length else b"{}"
        try:
            body = json.loads(raw)
        except ValueError:
            return {}
        return body if isinstance(body, dict) else {}

    def send_json(self, payload: object, status: int = 200) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def send_chunked(self, frames: list[bytes], content_type: str, delay: float) -> None:
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()
        try:
            for frame in frames:
                self.wfile.write(b"%x\r\n" % len(frame) + frame + b"\r\n")
                self.wfile.flush()
                time.sleep(delay)
            self.wfile.write(b"0\r\n\r\n")
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            # A cancelled stream closes the socket; that is the expected path.
            pass

    def do_POST(self) -> None:  # noqa: N802
        self.count_request()
        body = self.read_body()
        path = self.path.split("?", 1)[0]
        query = self.path.partition("?")[2]
        model = str(body.get("model") or body.get("deployment") or "")
        delay = SLOW_DELAY if model == "cancel-1" else CHUNK_DELAY
        streaming = body.get("stream") is True or "alt=sse" in query or path.endswith("-stream")

        if path.endswith("-stream"):
            # Bedrock relays legitimately answer with NDJSON; the core accepts it.
            frames = [ndjson({"messageStart": {"role": "assistant"}})]
            frames += [ndjson(bedrock_delta(i)) for i in range(CHUNK_FRAMES)]
            frames.append(ndjson({"messageStop": {"stopReason": "end_turn"}}))
            frames.append(
                ndjson({
                    "metadata": {
                        "usage": {
                            "inputTokens": INPUT_TOKENS,
                            "outputTokens": OUTPUT_TOKENS,
                            "totalTokens": INPUT_TOKENS + OUTPUT_TOKENS,
                        }
                    }
                })
            )
            self.send_chunked(frames, "application/x-ndjson", delay)
            return
        if path.endswith("/converse"):
            self.send_json({
                "output": {"message": {"role": "assistant", "content": [{"text": "soak reply"}]}},
                "stopReason": "end_turn",
                "usage": {
                    "inputTokens": INPUT_TOKENS,
                    "outputTokens": OUTPUT_TOKENS,
                    "totalTokens": INPUT_TOKENS + OUTPUT_TOKENS,
                },
            })
            return
        if path == "/api/chat":
            if streaming:
                frames = [
                    ndjson({"model": "soak", "message": {"role": "assistant", "content": f"chunk {i} "}, "done": False})
                    for i in range(CHUNK_FRAMES)
                ]
                frames.append(
                    ndjson({
                        "model": "soak",
                        "message": {"role": "assistant", "content": ""},
                        "done": True,
                        "done_reason": "stop",
                        "prompt_eval_count": INPUT_TOKENS,
                        "eval_count": OUTPUT_TOKENS,
                    })
                )
                self.send_chunked(frames, "application/x-ndjson", delay)
                return
            self.send_json({
                "model": "soak",
                "message": {"role": "assistant", "content": "soak reply"},
                "done": True,
                "done_reason": "stop",
                "prompt_eval_count": INPUT_TOKENS,
                "eval_count": OUTPUT_TOKENS,
            })
            return
        if path.endswith(":streamGenerateContent") or path.endswith(":generateContent"):
            if streaming:
                frames = [sse(gemini_frame(i)) for i in range(CHUNK_FRAMES)]
                frames.append(
                    sse({
                        "candidates": [{"content": {"parts": []}, "finishReason": "STOP", "index": 0}],
                        "usageMetadata": {
                            "promptTokenCount": INPUT_TOKENS,
                            "candidatesTokenCount": OUTPUT_TOKENS,
                            "totalTokenCount": INPUT_TOKENS + OUTPUT_TOKENS,
                        },
                    })
                )
                self.send_chunked(frames, "text/event-stream", delay)
                return
            self.send_json({
                "candidates": [
                    {"content": {"parts": [{"text": "soak reply"}]}, "finishReason": "STOP", "index": 0}
                ],
                "usageMetadata": {
                    "promptTokenCount": INPUT_TOKENS,
                    "candidatesTokenCount": OUTPUT_TOKENS,
                    "totalTokenCount": INPUT_TOKENS + OUTPUT_TOKENS,
                },
            })
            return
        if path.endswith("/messages"):
            if streaming:
                frames = [
                    sse_named(
                        "message_start",
                        {
                            "type": "message_start",
                            "message": {
                                "id": "soak-1",
                                "type": "message",
                                "role": "assistant",
                                "content": [],
                                "usage": {"input_tokens": INPUT_TOKENS, "output_tokens": 0},
                            },
                        },
                    ),
                    sse_named(
                        "content_block_start",
                        {"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}},
                    ),
                ]
                frames += [
                    sse_named(
                        "content_block_delta",
                        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": f"chunk {i} "}},
                    )
                    for i in range(CHUNK_FRAMES)
                ]
                frames.append(sse_named("content_block_stop", {"type": "content_block_stop", "index": 0}))
                frames.append(
                    sse_named(
                        "message_delta",
                        {
                            "type": "message_delta",
                            "delta": {"stop_reason": "end_turn"},
                            "usage": {"output_tokens": OUTPUT_TOKENS},
                        },
                    )
                )
                frames.append(sse_named("message_stop", {"type": "message_stop"}))
                self.send_chunked(frames, "text/event-stream", delay)
                return
            self.send_json({
                "id": "soak-1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "soak reply"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": INPUT_TOKENS, "output_tokens": OUTPUT_TOKENS},
            })
            return
        if path.endswith("/responses"):
            if streaming:
                frames = [
                    sse_named("response.created", {"type": "response.created", "response": {"id": "soak-1", "status": "in_progress"}}),
                ]
                frames += [
                    sse_named(
                        "response.output_text.delta",
                        {"type": "response.output_text.delta", "delta": f"chunk {i} ", "output_index": 0},
                    )
                    for i in range(CHUNK_FRAMES)
                ]
                frames.append(
                    sse_named(
                        "response.completed",
                        {
                            "type": "response.completed",
                            "response": {
                                "id": "soak-1",
                                "status": "completed",
                                "usage": {
                                    "input_tokens": INPUT_TOKENS,
                                    "output_tokens": OUTPUT_TOKENS,
                                    "total_tokens": INPUT_TOKENS + OUTPUT_TOKENS,
                                },
                            },
                        },
                    )
                )
                self.send_chunked(frames, "text/event-stream", delay)
                return
            self.send_json({
                "id": "soak-1",
                "status": "completed",
                "output": [
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": "soak reply"}],
                    }
                ],
                "usage": {
                    "input_tokens": INPUT_TOKENS,
                    "output_tokens": OUTPUT_TOKENS,
                    "total_tokens": INPUT_TOKENS + OUTPUT_TOKENS,
                },
            })
            return
        if path.endswith("/chat/completions") or path == "/v2/chat":
            if path == "/v2/chat":
                if streaming:
                    frames = [sse({"type": "message-start", "id": "soak-1"})]
                    frames += [
                        sse({"type": "content-delta", "index": 0, "delta": {"message": {"content": {"text": f"chunk {i} "}}}})
                        for i in range(CHUNK_FRAMES)
                    ]
                    frames.append(
                        sse({
                            "type": "message-end",
                            "delta": {
                                "finish_reason": "COMPLETE",
                                "usage": {
                                    "tokens": {
                                        "input_tokens": INPUT_TOKENS,
                                        "output_tokens": OUTPUT_TOKENS,
                                    }
                                },
                            },
                        })
                    )
                    self.send_chunked(frames, "text/event-stream", delay)
                    return
                self.send_json({
                    "id": "soak-1",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "soak reply"}],
                    },
                    "finish_reason": "COMPLETE",
                    "usage": {"tokens": {"input_tokens": INPUT_TOKENS, "output_tokens": OUTPUT_TOKENS}},
                })
                return
            if streaming:
                frames = [sse(openai_chunk(i)) for i in range(CHUNK_FRAMES)]
                frames.append(
                    sse({
                        "id": "soak-1",
                        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
                        "usage": {
                            "prompt_tokens": INPUT_TOKENS,
                            "completion_tokens": OUTPUT_TOKENS,
                            "total_tokens": INPUT_TOKENS + OUTPUT_TOKENS,
                        },
                    })
                )
                frames.append(b"data: [DONE]\n\n")
                self.send_chunked(frames, "text/event-stream", delay)
                return
            self.send_json({
                "id": "soak-1",
                "model": "soak",
                "choices": [
                    {"message": {"role": "assistant", "content": "soak reply"}, "finish_reason": "stop"}
                ],
                "usage": {
                    "prompt_tokens": INPUT_TOKENS,
                    "completion_tokens": OUTPUT_TOKENS,
                    "total_tokens": INPUT_TOKENS + OUTPUT_TOKENS,
                },
            })
            return
        if path.endswith("/rerank"):
            self.send_json({"results": [{"index": 1, "relevance_score": 0.9}]})
            return
        if path.endswith("/embeddings") or path.endswith("/api/embed") or path.endswith("/v2/embed"):
            self.send_json({
                "data": [{"embedding": [0.1, 0.2, 0.3]}, {"embedding": [0.4, 0.5, 0.6]}],
                "usage": {"prompt_tokens": 4, "total_tokens": 4},
            })
            return
        self.send_json({"error": "unknown_endpoint"}, status=404)

    def do_GET(self) -> None:  # noqa: N802
        if self.path == "/healthz":
            self.send_json({"status": "ok"})
            return
        self.send_json({"error": "not_found"}, status=404)


def main() -> int:
    parser = argparse.ArgumentParser(description="LingXi soak upstream")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    class QuietServer(ThreadingHTTPServer):
        daemon_threads = True

        def handle_error(self, request, client_address) -> None:  # noqa: ANN001
            # Cancelled streams reset the socket; that is the expected path.
            return

    server = QuietServer((args.host, args.port), SoakHandler)
    print(json.dumps({"url": f"http://{args.host}:{server.server_port}"}), flush=True)

    def report(_signum: int, _frame: object) -> None:
        # Leave the accepted-connection count behind: the driver cannot see it,
        # and a soaking client that leaks sockets shows up here as growth.
        print(
            json.dumps({
                "final": True,
                "requests": SoakHandler.requests_served,
                "connections": len(SoakHandler.connections_seen),
            }),
            flush=True,
        )
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGTERM, report)
    signal.signal(signal.SIGINT, report)
    try:
        server.serve_forever()
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())