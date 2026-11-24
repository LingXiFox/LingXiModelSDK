import http.server
import json
import os
import threading
import unittest
from pathlib import Path

from lingxi_model_sdk.ffi_bridge import RustCoreBridge
from lingxi_model_sdk.native import NativeError
from lingxi_model_sdk.registry import ProviderRegistry
from lingxi_model_sdk.unified import UnifiedClient, UnsupportedFeatureError

FFI_READY = RustCoreBridge().available or os.environ.get("LINGXI_REQUIRE_FFI") == "1"

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "conformance" / "native"


class _MockHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):  # noqa: A002
        return

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        _ = self.rfile.read(length)
        if self.path.startswith("/redir"):
            self.send_response(302)
            self.send_header("Location", "http://127.0.0.1:9/leak")
            self.end_headers()
            return
        content_type = "application/json"
        if self.path.endswith("/converse-stream"):
            assert self.headers.get("Authorization", "").startswith("AWS4-HMAC-SHA256")
            content_type = "application/vnd.amazon.eventstream"
            body = (FIXTURES / "aws_bedrock_stream.eventstream").read_bytes()
        elif "converse" in self.path:
            assert self.headers.get("Authorization", "").startswith("AWS4-HMAC-SHA256")
            body = (FIXTURES / "aws_bedrock_response.json").read_bytes()
        elif "generateContent" in self.path:
            body = (FIXTURES / "google_vertex_response.json").read_bytes()
        elif "/deployments/" in self.path:
            body = (FIXTURES / "azure_openai_response.json").read_bytes()
        elif "/api/chat" in self.path:
            body = (FIXTURES / "ollama_chat_response.json").read_bytes()
        elif "/v2/rerank" in self.path:
            body = (FIXTURES / "cohere_rerank_response.json").read_bytes()
        elif "/v2/chat" in self.path:
            body = (FIXTURES / "cohere_chat_response.json").read_bytes()
        elif "/embeddings" in self.path:
            body = (FIXTURES / "openai_embed_response.json").read_bytes()
        else:
            body = (FIXTURES / "openai_chat_response.json").read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class TestPythonNativeAndUnified(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _MockHandler)
        cls.port = cls.server.server_address[1]
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base = f"http://127.0.0.1:{cls.port}"

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()

    def test_registry_loads_all_providers(self) -> None:
        reg = ProviderRegistry.from_json((ROOT / "registry" / "registry.json").read_text(encoding="utf-8"))
        self.assertGreaterEqual(len(reg), 320)

    @unittest.skipUnless(FFI_READY, "liblingxi_core not built in catalog-only environment")
    def test_nine_protocols_embed_rerank_and_security(self) -> None:
        client = UnifiedClient()
        self.assertTrue(client.bridge.available, "liblingxi_core must be loaded via ctypes")
        req = {
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}],
        }
        b_out = client.complete("aws_bedrock", self.base, "AKIA:SECRET", req)
        self.assertEqual(b_out["content"][0]["signature"], "sig-bedrock-1")

        events = client.stream("aws_bedrock", self.base, "AKIA:SECRET", req)
        self.assertIn("tool_call_delta", {event["type"] for event in events})
        self.assertTrue(any("東京" in event.get("arguments_delta", "") for event in events))

        v_out = client.complete("google_vertex_ai", self.base, "tok", req)
        self.assertEqual(v_out["content"][0]["signature"], "sig-vertex-1")

        az_out = client.complete("azure_openai", self.base, "az-key", req)
        self.assertIn("Azure", az_out["content"][0]["text"])

        ol_out = client.complete("ollama", self.base, None, req)
        self.assertEqual(ol_out["usage"]["totalTokens"], 22)

        co_out = client.complete("cohere", self.base, "co-key", req)
        self.assertEqual(co_out["finishReason"], "COMPLETE")

        emb = client.embed("openai_chat_completions", self.base, "sk-test", "text-embedding-3-small", ["a", "b"])
        self.assertEqual(len(emb["embeddings"]), 2)

        rr = client.rerank("cohere", self.base, "co-key", "rerank-v3.5", "py", ["a", "b"], top_n=2)
        self.assertEqual(rr["results"][0]["index"], 1)

        with self.assertRaises(UnsupportedFeatureError):
            client.rerank("openai_chat_completions", self.base, "sk-test", "m", "q", ["a"])

        with self.assertRaises(NativeError):
            client.complete("openai_chat_completions", f"{self.base}/redir", "sk-secret", req)


if __name__ == "__main__":
    unittest.main()
