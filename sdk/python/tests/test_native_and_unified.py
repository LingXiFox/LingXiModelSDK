import http.server
import json
import os
import threading
import time
import unittest
from pathlib import Path

from lingxi_model_sdk.ffi_bridge import RustCoreBridge
from lingxi_model_sdk.native import NativeError, NativeRequestConfig
from lingxi_model_sdk.registry import ProviderRegistry
from lingxi_model_sdk.unified import UnifiedClient, UnsupportedFeatureError

FFI_READY = RustCoreBridge().available or os.environ.get("LINGXI_REQUIRE_FFI") == "1"

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "conformance" / "native"


def _fixture(name: str) -> bytes:
    return (FIXTURES / name).read_bytes()


def _blocks(response: dict, block_type: str) -> list[dict]:
    return [block for block in response.get("content", []) if block.get("type") == block_type]


class _MockHandler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"

    def log_message(self, format, *args):  # noqa: A002
        return

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        compact = body.replace(b" ", b"")
        streaming = (
            b'"stream":true' in compact
            or "alt=sse" in self.path
            or self.path.endswith("-stream")
        )
        suffix = "-stream" if streaming else ""
        if self.path.startswith("/redir"):
            self.send_response(302)
            self.send_header("Location", "http://127.0.0.1:9/leak")
            self.end_headers()
            return
        if self.path.endswith("/converse-stream") or self.path.endswith("/converse"):
            assert self.headers.get("Authorization", "").startswith("AWS4-HMAC-SHA256")
            if streaming:
                payload, content_type = _fixture("aws_bedrock_stream.eventstream"), "application/vnd.amazon.eventstream"
            else:
                payload, content_type = _fixture("aws_bedrock_response.json"), "application/json"
        elif "/deployments/" in self.path:
            payload = _fixture("azure_openai_response.json") if not streaming else _fixture("azure_openai_stream.sse")
            content_type = "text/event-stream" if streaming else "application/json"
        elif "generatecontent" in self.path.lower():
            vertex = "/publishers/" in self.path
            if streaming:
                payload = _fixture("google_vertex_stream.sse" if vertex else "google_gemini_stream.sse")
            else:
                payload = _fixture("google_vertex_response.json" if vertex else "google_gemini_response.json")
            content_type = "text/event-stream" if streaming else "application/json"
        elif "/api/chat" in self.path:
            payload = _fixture("ollama_chat_stream.ndjson" if streaming else "ollama_chat_response.json")
            content_type = "application/x-ndjson" if streaming else "application/json"
        elif "/v2/rerank" in self.path:
            payload, content_type = _fixture("cohere_rerank_response.json"), "application/json"
        elif "/v2/chat" in self.path:
            payload = _fixture("cohere_chat_stream.sse" if streaming else "cohere_chat_response.json")
            content_type = "text/event-stream" if streaming else "application/json"
        elif "/embeddings" in self.path:
            payload, content_type = _fixture("openai_embed_response.json"), "application/json"
        elif self.path.endswith("/messages"):
            payload = _fixture("anthropic_messages_stream.sse" if streaming else "anthropic_messages_response.json")
            content_type = "text/event-stream" if streaming else "application/json"
        elif self.path.endswith("/responses"):
            payload = _fixture("openai_responses_stream.sse" if streaming else "openai_responses_response.json")
            content_type = "text/event-stream" if streaming else "application/json"
        else:
            payload = _fixture("openai_chat_stream.sse" if streaming else "openai_chat_response.json")
            content_type = "text/event-stream" if streaming else "application/json"
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        try:
            self.wfile.write(payload)
            self.wfile.flush()
        except BrokenPipeError:
            return


_SLOW_FRAMES = [
    b'data: {"choices":[{"delta":{"content":"slow "}}]}\n\n',
    b'data: {"choices":[{"delta":{"content":"moving "}}]}\n\n',
    b'data: {"choices":[{"delta":{"content":"stream"}}]}\n\n',
    b"data: [DONE]\n\n",
]


class _SlowHandler(http.server.BaseHTTPRequestHandler):
    """Streams one SSE frame every 200 ms so latency is observable."""

    protocol_version = "HTTP/1.0"

    def log_message(self, format, *args):  # noqa: A002
        return

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(length)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            for index, frame in enumerate(_SLOW_FRAMES):
                self.wfile.write(frame)
                self.wfile.flush()
                if index + 1 < len(_SLOW_FRAMES):
                    time.sleep(0.2)
        except BrokenPipeError:
            # The client cancelled the stream; that is the behaviour under test.
            return


class TestPythonNativeAndUnified(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _MockHandler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f"http://127.0.0.1:{cls.server.server_address[1]}"

        cls.slow = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _SlowHandler)
        cls.slow_thread = threading.Thread(target=cls.slow.serve_forever, daemon=True)
        cls.slow_thread.start()
        cls.slow_base = f"http://127.0.0.1:{cls.slow.server_address[1]}"

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.slow.shutdown()
        cls.slow.server_close()

    def test_registry_loads_all_providers(self) -> None:
        reg = ProviderRegistry.from_json((ROOT / "registry" / "registry.json").read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(reg), 320)

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_unified_returns_lmp_for_every_protocol(self) -> None:
        client = UnifiedClient()
        self.assertTrue(client.bridge.available, "liblingxi_core must be loaded via ctypes")
        request = {
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        }

        chat = client.complete("openai_chat_completions", self.base, "sk-test", request)
        self.assertEqual(chat["finishReason"], "stop")
        self.assertEqual([b["text"] for b in _blocks(chat, "text")], ["Hello from chat completions"])
        self.assertEqual([b["thinking"] for b in _blocks(chat, "thinking")], ["step-by-step"])

        responses = client.complete("openai_responses", self.base, "sk-test", request)
        self.assertEqual([b["text"] for b in _blocks(responses, "text")], ["Hello from responses"])
        self.assertEqual(responses["usage"]["totalTokens"], 23)

        anthropic = client.complete("anthropic_messages", self.base, "sk-ant", request)
        self.assertEqual(_blocks(anthropic, "thinking")[0]["signature"], "sig_abc")
        self.assertEqual(anthropic["usage"]["cacheReadTokens"], 5)

        gemini = client.complete("google_gemini", self.base, "goog", request)
        self.assertEqual(gemini["finishReason"], "STOP")

        bedrock = client.complete("aws_bedrock", self.base, "AKIA:SECRET", request)
        self.assertEqual(_blocks(bedrock, "thinking")[0]["signature"], "sig-bedrock-1")
        self.assertNotIn("output", bedrock)

        vertex = client.complete("google_vertex_ai", self.base, "tok", request)
        self.assertEqual(_blocks(vertex, "thinking")[0]["signature"], "sig-vertex-1")

        azure = client.complete("azure_openai", self.base, "az-key", request)
        self.assertIn("Azure", "".join(b["text"] for b in _blocks(azure, "text")))

        ollama = client.complete("ollama", self.base, None, request)
        self.assertEqual(ollama["usage"]["totalTokens"], 22)

        cohere = client.complete("cohere", self.base, "co-key", request)
        self.assertEqual(cohere["finishReason"], "COMPLETE")

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_unified_streams_incrementally(self) -> None:
        client = UnifiedClient()
        request = {"model": "m", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}

        events = client.stream_all("openai_chat_completions", self.base, "sk-test", request)
        text = "".join(e.get("delta", "") for e in events if e.get("type") == "text_delta")
        self.assertEqual(text, "Hello world")
        self.assertIn("message_end", {e.get("type") for e in events})

        bedrock = client.stream_all("aws_bedrock", self.base, "AKIA:SECRET", request)
        self.assertIn("tool_call_delta", {e.get("type") for e in bedrock})
        self.assertTrue(any("東京" in e.get("argumentsDelta", "") for e in bedrock))

        gemini = client.stream_all("google_gemini", self.base, "goog", request)
        gemini_text = "".join(e.get("delta", "") for e in gemini if e.get("type") == "text_delta")
        self.assertEqual(gemini_text, "Hello Gemini")

        anthropic = client.stream_all("anthropic_messages", self.base, "sk-ant", request)
        anthropic_text = "".join(e.get("delta", "") for e in anthropic if e.get("type") == "text_delta")
        self.assertEqual(anthropic_text, "Hello Anthropic")

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_stream_events_arrive_before_the_upstream_finishes(self) -> None:
        client = UnifiedClient()
        request = {"model": "m", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}
        started = time.monotonic()
        stream = client.stream("openai_chat_completions", self.slow_base, "sk-test", request)
        try:
            first = stream.next_event()
            first_delta_at = time.monotonic() - started
            self.assertIsNotNone(first)
            remaining = list(stream)
        finally:
            stream.close()
        elapsed = time.monotonic() - started
        # The core pays a one-off cost to spin up the stream runtime; what matters
        # is that the first event lands well before the final frame is written.
        self.assertLess(first_delta_at, elapsed - 0.3, "events must not wait for the whole body")
        self.assertGreaterEqual(elapsed, 0.5, "the scripted stream still takes its own time")
        self.assertTrue(remaining)

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_stream_cancel_releases_the_connection(self) -> None:
        client = UnifiedClient()
        request = {"model": "m", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}
        stream = client.stream("openai_chat_completions", self.slow_base, "sk-test", request)
        self.assertIsNotNone(stream.next_event())
        stream.cancel()
        stream.close()
        self.assertTrue(stream.closed)

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_embed_rerank_and_unsupported_features(self) -> None:
        client = UnifiedClient()
        embedded = client.embed(
            "openai_chat_completions", self.base, "sk-test", "text-embedding-3-small", ["a", "b"]
        )
        self.assertEqual(len(embedded["embeddings"]), 2)
        self.assertEqual(embedded["embeddings"][0], [0.1, 0.2, 0.3])

        reranked = client.rerank("cohere", self.base, "co-key", "rerank-v3.5", "py", ["a", "b"], top_n=2)
        self.assertEqual(reranked["results"][0]["index"], 1)
        self.assertAlmostEqual(reranked["results"][0]["relevanceScore"], 0.94)

        with self.assertRaises(UnsupportedFeatureError) as ctx:
            client.embed("anthropic_messages", self.base, "k", "m", ["a"])
        self.assertEqual(ctx.exception.feature, "embed")

        with self.assertRaises(UnsupportedFeatureError):
            client.rerank("openai_chat_completions", self.base, "sk-test", "m", "q", ["a"])

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_native_transport_blocks_redirects_and_uses_the_core(self) -> None:
        client = UnifiedClient()
        request = {"model": "m", "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]}
        with self.assertRaises(NativeError):
            client.complete("openai_chat_completions", f"{self.base}/redir", "sk-secret", request)

        native = client.native
        target = native.prepare(
            "aws_bedrock",
            NativeRequestConfig(base_url=self.base, api_key="AKID:SECRET"),
            "anthropic.claude-3-5-sonnet",
            {"messages": []},
            stream=True,
        )
        self.assertTrue(target.headers["Authorization"].startswith("AWS4-HMAC-SHA256 Credential=AKID/"))
        self.assertNotIn("SECRET", target.headers["Authorization"])
        self.assertEqual(target.framing, "amazon_event_stream")

        anthropic = native.prepare(
            "anthropic_messages",
            NativeRequestConfig(base_url="https://api.anthropic.com", api_key="sk-ant-test"),
            "claude-sonnet-4",
            {"messages": []},
        )
        self.assertEqual(anthropic.url, "https://api.anthropic.com/v1/messages")
        self.assertEqual(anthropic.headers["anthropic-version"], "2023-06-01")

        with self.assertRaises(NativeError):
            native.prepare(
                "openai_chat_completions",
                NativeRequestConfig(base_url="http://example.com/v1", api_key="sk"),
                "gpt-4o",
                {},
            )


if __name__ == "__main__":
    unittest.main()
