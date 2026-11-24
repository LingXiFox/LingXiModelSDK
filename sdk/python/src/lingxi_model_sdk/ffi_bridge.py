"""Optional ctypes bridge to the Rust lingxi_core shared library."""

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


class RustCoreBridge:
    def __init__(self) -> None:
        self.lib_path = _find_library()
        self._lib: ctypes.CDLL | None = None
        if self.lib_path is not None:
            lib = ctypes.CDLL(str(self.lib_path))
            for fn_name in (
                "lingxi_lmp_translate_request",
                "lingxi_lmp_normalize_response",
                "lingxi_lmp_normalize_stream_events",
            ):
                fn = getattr(lib, fn_name)
                fn.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
                fn.restype = ctypes.c_void_p
            lib.lingxi_string_free.argtypes = [ctypes.c_void_p]
            lib.lingxi_string_free.restype = None
            self._lib = lib

    @property
    def available(self) -> bool:
        return self._lib is not None

    def _call(self, fn_name: str, arg1: str, arg2: str) -> dict[str, Any]:
        assert self._lib is not None
        fn = getattr(self._lib, fn_name)
        ptr = fn(arg1.encode("utf-8"), arg2.encode("utf-8"))
        try:
            raw = ctypes.cast(ptr, ctypes.c_char_p).value.decode("utf-8")
        finally:
            self._lib.lingxi_string_free(ptr)
        return json.loads(raw)

    def translate_request(self, protocol: str, req: dict[str, Any]) -> dict[str, Any]:
        res = self._call("lingxi_lmp_translate_request", protocol, json.dumps(req))
        if not res.get("ok"):
            raise RuntimeError(res.get("error", "translate_request failed"))
        return res["payload"]

    def normalize_response(self, protocol: str, raw_resp: dict[str, Any]) -> dict[str, Any]:
        res = self._call("lingxi_lmp_normalize_response", protocol, json.dumps(raw_resp))
        if not res.get("ok"):
            raise RuntimeError(res.get("error", "normalize_response failed"))
        return res["response"]

    def normalize_stream_events(self, protocol: str, events: list[dict[str, Any]]) -> list[dict[str, Any]]:
        res = self._call("lingxi_lmp_normalize_stream_events", protocol, json.dumps(events))
        if not res.get("ok"):
            raise RuntimeError(res.get("error", "normalize_stream_events failed"))
        return res["events"]
