"""ctypes bridge to the Rust lingxi_core shared library.

The bridge is the single place where provider protocol wiring lives: URL
joining, authentication headers, SigV4 signing, payload translation and LMP
normalization all happen in the Rust core. Python only owns the transport
(urllib) and the ergonomics.

Every call marshals JSON through the C ABI, so a Python user sees exactly the
same normalization results as the Swift, Java, Kotlin, C#, Dart and C++ SDKs.
"""

from __future__ import annotations

import ctypes
import json
import os
import sys
from pathlib import Path
from typing import Any


def _find_library() -> Path | None:
    env_path = os.environ.get("LINGXI_CORE_LIB")
    if env_path and Path(env_path).is_file():
        return Path(env_path)
    ext = "dylib" if sys.platform == "darwin" else "so" if sys.platform.startswith("linux") else "dll"
    name = f"liblingxi_core.{ext}" if ext != "dll" else "lingxi_core.dll"
    here = Path(__file__).resolve()
    for parent in here.parents:
        for rel in (f"core/target/release/{name}", f"core/target/debug/{name}"):
            cand = parent / rel
            if cand.is_file():
                return cand
    return None


class CoreError(RuntimeError):
    """Typed failure raised by the Rust core.

    `code` mirrors `LmpError::code()` in the core, so callers branch on
    `unsupported_feature` / `auth` / `invalid_endpoint` instead of parsing
    prose.
    """

    def __init__(self, operation: str, message: str, code: str | None = None) -> None:
        suffix = f" [{code}]" if code else ""
        super().__init__(f"{operation} failed{suffix}: {message}")
        self.operation = operation
        self.code = code
        self.message = message


class RustCoreBridge:
    """Thin, typed wrapper over the stable C ABI of `lingxi_core`."""

    _PAIR_FUNCTIONS = (
        "lingxi_lmp_translate_request",
        "lingxi_lmp_normalize_response",
        "lingxi_lmp_normalize_stream_events",
    )
    _JSON_FUNCTIONS = (
        "lingxi_native_prepare",
        "lingxi_unified_prepare",
        "lingxi_unified_embed",
        "lingxi_unified_rerank",
    )

    def __init__(self) -> None:
        self.lib_path = _find_library()
        self._lib: ctypes.CDLL | None = None
        self._missing: set[str] = set()
        if self.lib_path is None:
            return
        lib = ctypes.CDLL(str(self.lib_path))
        for fn_name in self._PAIR_FUNCTIONS:
            if not hasattr(lib, fn_name):
                self._missing.add(fn_name)
                continue
            fn = getattr(lib, fn_name)
            fn.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
            fn.restype = ctypes.c_void_p
        for fn_name in self._JSON_FUNCTIONS:
            if not hasattr(lib, fn_name):
                self._missing.add(fn_name)
                continue
            fn = getattr(lib, fn_name)
            fn.argtypes = [ctypes.c_char_p]
            fn.restype = ctypes.c_void_p
        for fn_name, argtypes, restype in (
            ("lingxi_stream_open", [ctypes.c_char_p], ctypes.c_void_p),
            ("lingxi_stream_open_error", [], ctypes.c_void_p),
            ("lingxi_stream_next", [ctypes.c_void_p, ctypes.c_ulonglong], ctypes.c_void_p),
            ("lingxi_stream_cancel", [ctypes.c_void_p], None),
            ("lingxi_stream_close", [ctypes.c_void_p], None),
            ("lingxi_stream_last_error", [ctypes.c_void_p], ctypes.c_void_p),
            ("lingxi_lmp_normalizer_open", [ctypes.c_char_p], ctypes.c_void_p),
            ("lingxi_normalizer_open_error", [], ctypes.c_void_p),
            ("lingxi_lmp_normalizer_push", [ctypes.c_void_p, ctypes.c_char_p], ctypes.c_void_p),
            ("lingxi_lmp_normalizer_finish", [ctypes.c_void_p], ctypes.c_void_p),
            ("lingxi_lmp_normalizer_close", [ctypes.c_void_p], None),
        ):
            if not hasattr(lib, fn_name):
                self._missing.add(fn_name)
                continue
            fn = getattr(lib, fn_name)
            fn.argtypes = argtypes
            fn.restype = restype
        if not hasattr(lib, "lingxi_string_free"):
            self._missing.add("lingxi_string_free")
        else:
            lib.lingxi_string_free.argtypes = [ctypes.c_void_p]
            lib.lingxi_string_free.restype = None
        self._lib = lib

    @property
    def available(self) -> bool:
        return self._lib is not None and not self._missing

    @property
    def missing_symbols(self) -> set[str]:
        return set(self._missing)

    def _require(self, fn_name: str) -> None:
        if self._lib is None:
            raise RuntimeError(
                "lingxi_core shared library not found; build it with "
                "'cargo build --manifest-path core/Cargo.toml' or set LINGXI_CORE_LIB"
            )
        if fn_name in self._missing:
            raise RuntimeError(
                f"lingxi_core is missing {fn_name}; rebuild it with: "
                "cargo build --manifest-path core/Cargo.toml"
            )

    def _take(self, ptr: int) -> dict[str, Any]:
        assert self._lib is not None
        try:
            raw = ctypes.cast(ptr, ctypes.c_char_p).value.decode("utf-8")
        finally:
            self._lib.lingxi_string_free(ptr)
        return json.loads(raw)

    def _call_json(self, fn_name: str, payload: dict[str, Any]) -> dict[str, Any]:
        self._require(fn_name)
        assert self._lib is not None
        fn = getattr(self._lib, fn_name)
        ptr = fn(json.dumps(payload).encode("utf-8"))
        if not ptr:
            raise CoreError(fn_name, "core returned no result")
        return self._take(ptr)

    def _call_pair(self, fn_name: str, protocol: str, payload: Any) -> dict[str, Any]:
        self._require(fn_name)
        assert self._lib is not None
        fn = getattr(self._lib, fn_name)
        ptr = fn(protocol.encode("utf-8"), json.dumps(payload).encode("utf-8"))
        if not ptr:
            raise CoreError(fn_name, "core returned no result")
        return self._take(ptr)

    def _require_ok(self, operation: str, envelope: dict[str, Any]) -> dict[str, Any]:
        if not envelope.get("ok"):
            raise CoreError(operation, envelope.get("error", "unknown error"), envelope.get("code"))
        return envelope

    # -- request and response shaping --------------------------------------

    def translate_request(self, protocol: str, req: dict[str, Any]) -> dict[str, Any]:
        env = self._require_ok(
            "translate_request", self._call_pair("lingxi_lmp_translate_request", protocol, req)
        )
        return env["payload"]

    def normalize_response(self, protocol: str, raw_resp: dict[str, Any]) -> dict[str, Any]:
        env = self._require_ok(
            "normalize_response",
            self._call_pair("lingxi_lmp_normalize_response", protocol, raw_resp),
        )
        return env["response"]

    def normalize_stream_events(
        self, protocol: str, events: list[dict[str, Any]]
    ) -> list[dict[str, Any]]:
        env = self._require_ok(
            "normalize_stream_events",
            self._call_pair("lingxi_lmp_normalize_stream_events", protocol, events),
        )
        return env["events"]

    # -- wire targets ------------------------------------------------------

    def prepare_native(self, request: dict[str, Any]) -> dict[str, Any]:
        return self._require_ok("prepare_native", self._call_json("lingxi_native_prepare", request))

    def prepare_unified(self, request: dict[str, Any]) -> dict[str, Any]:
        env = self._require_ok(
            "prepare_unified", self._call_json("lingxi_unified_prepare", request)
        )
        return env

    # -- embeddings and reranking ------------------------------------------

    def embed(self, request: dict[str, Any]) -> dict[str, Any]:
        env = self._require_ok("embed", self._call_json("lingxi_unified_embed", request))
        return env["response"]

    def rerank(self, request: dict[str, Any]) -> dict[str, Any]:
        env = self._require_ok("rerank", self._call_json("lingxi_unified_rerank", request))
        return env["response"]

    # -- incremental streaming ---------------------------------------------

    def open_stream(self, request: dict[str, Any]) -> "StreamHandle | None":
        self._require("lingxi_stream_open")
        assert self._lib is not None
        ptr = self._lib.lingxi_stream_open(json.dumps(request).encode("utf-8"))
        if not ptr:
            return None
        return StreamHandle(self, ptr)

    def stream_open_error(self) -> str:
        self._require("lingxi_stream_open_error")
        assert self._lib is not None
        ptr = self._lib.lingxi_stream_open_error()
        if not ptr:
            return "stream_open failed"
        try:
            return ctypes.cast(ptr, ctypes.c_char_p).value.decode("utf-8")
        finally:
            self._lib.lingxi_string_free(ptr)


class StreamHandle:
    """Consumer-driven stream handle.

    `next_event()` only reads when asked, which is the backpressure signal:
    a slow consumer stops pulling and the socket stops being drained. Call
    `close()` when finished (it is idempotent and NULL-safe).
    """

    def __init__(self, bridge: RustCoreBridge, handle: int) -> None:
        self._bridge = bridge
        self._handle: int | None = handle
        self._done = False

    def next_event(self, timeout_ms: int = 600_000) -> dict[str, Any] | None:
        if self._handle is None:
            return None
        bridge = self._bridge
        assert bridge._lib is not None
        ptr = bridge._lib.lingxi_stream_next(self._handle, timeout_ms)
        if not ptr:
            raise CoreError("stream_next", "core returned no result")
        envelope = bridge._take(ptr)
        if envelope.get("ok") and envelope.get("done"):
            self._done = True
            return None
        if not envelope.get("ok"):
            self._done = True
            raise CoreError("stream_next", envelope.get("error", "unknown error"), envelope.get("code"))
        return envelope["event"]

    def cancel(self) -> None:
        if self._handle is None:
            return
        bridge = self._bridge
        assert bridge._lib is not None
        bridge._lib.lingxi_stream_cancel(self._handle)

    def close(self) -> None:
        if self._handle is None:
            return
        handle = self._handle
        self._handle = None
        bridge = self._bridge
        assert bridge._lib is not None
        bridge._lib.lingxi_stream_close(handle)

    @property
    def closed(self) -> bool:
        return self._handle is None

    @property
    def finished(self) -> bool:
        return self._done

    def __enter__(self) -> "StreamHandle":
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:  # pragma: no cover - interpreter shutdown
            pass

    def __iter__(self) -> "StreamHandle":
        return self

    def __next__(self) -> dict[str, Any]:
        event = self.next_event()
        if event is None:
            raise StopIteration
        return event
