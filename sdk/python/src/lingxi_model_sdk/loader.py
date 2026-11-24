"""HTTP fetching, ETag revalidation, and memory/disk caching."""

from __future__ import annotations

import asyncio
import gzip
import json
import os
import tempfile
import threading
import urllib.error
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Protocol

from .catalog import LingXiModelCatalog
from .decoder import decode_catalog
from .errors import (
    ModelCatalogError,
    TransportError,
    UnavailableError,
    UnexpectedStatusError,
)

DEFAULT_ENDPOINT = "https://models.lingxifox.cn/models.json"


@dataclass(frozen=True)
class ModelCatalogConfiguration:
    endpoint: str = DEFAULT_ENDPOINT
    max_age_seconds: float = 1800.0
    cache_directory: Path | str | None = None
    request_timeout_seconds: float = 30.0
    user_agent: str = "LingXiModelSDK-Python/2.0"


@dataclass(frozen=True)
class CatalogHTTPResponse:
    status_code: int
    body: bytes
    etag: str | None = None


class CatalogTransport(Protocol):
    def fetch(self, url: str, headers: dict[str, str], timeout: float) -> CatalogHTTPResponse:
        ...


class UrllibCatalogTransport:
    def fetch(self, url: str, headers: dict[str, str], timeout: float) -> CatalogHTTPResponse:
        req = urllib.request.Request(url, headers=headers, method="GET")
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                raw = resp.read()
                if resp.headers.get("Content-Encoding", "").lower() == "gzip":
                    raw = gzip.decompress(raw)
                return CatalogHTTPResponse(
                    status_code=resp.getcode() or 200,
                    body=raw,
                    etag=resp.headers.get("ETag"),
                )
        except urllib.error.HTTPError as exc:
            return CatalogHTTPResponse(
                status_code=exc.code,
                body=b"",
                etag=exc.headers.get("ETag") if exc.headers else None,
            )
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            raise TransportError(str(exc)) from exc


@dataclass(frozen=True)
class CachedModelCatalog:
    catalog: LingXiModelCatalog
    fetched_at: datetime
    etag: str | None = None

    def is_fresh(self, max_age_seconds: float, now: datetime | None = None) -> bool:
        if max_age_seconds <= 0:
            return False
        current = now or datetime.now(timezone.utc)
        return (current - self.fetched_at).total_seconds() < max_age_seconds


class ModelCatalogLoader:
    def __init__(
        self,
        configuration: ModelCatalogConfiguration | None = None,
        transport: CatalogTransport | None = None,
    ) -> None:
        self.configuration = configuration or ModelCatalogConfiguration()
        self.transport = transport or UrllibCatalogTransport()
        self._lock = threading.Lock()
        self._memory: CachedModelCatalog | None = None

    def _cache_dir(self) -> Path | None:
        d = self.configuration.cache_directory
        return Path(d) if d is not None else None

    def cached(self) -> CachedModelCatalog | None:
        with self._lock:
            return self._cached_unlocked()

    def _cached_unlocked(self) -> CachedModelCatalog | None:
        if self._memory is not None:
            return self._memory
        cache_dir = self._cache_dir()
        if cache_dir is None:
            return None
        catalog_path = cache_dir / "models.json"
        if not catalog_path.exists():
            return None
        try:
            raw = catalog_path.read_bytes()
            cat = decode_catalog(raw)
        except (OSError, ModelCatalogError):
            return None

        etag: str | None = None
        fetched_at = cat.revision.generated_at or datetime.fromtimestamp(0, tz=timezone.utc)
        meta_path = cache_dir / "models.json.meta.json"
        if meta_path.exists():
            try:
                meta = json.loads(meta_path.read_text(encoding="utf-8"))
                if isinstance(meta, dict):
                    etag = meta.get("etag") if isinstance(meta.get("etag"), str) else None
                    ts = meta.get("fetchedAt")
                    if isinstance(ts, str):
                        fetched_at = datetime.fromisoformat(ts.replace("Z", "+00:00"))
            except (OSError, ValueError):
                pass
        self._memory = CachedModelCatalog(catalog=cat, fetched_at=fetched_at, etag=etag)
        return self._memory

    def load(self, force_refresh: bool = False) -> LingXiModelCatalog:
        with self._lock:
            cached = self._cached_unlocked()
            now = datetime.now(timezone.utc)
            if not force_refresh and cached is not None and cached.is_fresh(self.configuration.max_age_seconds, now):
                return cached.catalog

            headers = {
                "Accept": "application/json",
                "Accept-Encoding": "gzip",
                "User-Agent": self.configuration.user_agent,
            }
            if cached is not None and cached.etag:
                headers["If-None-Match"] = cached.etag

            try:
                resp = self.transport.fetch(
                    self.configuration.endpoint,
                    headers,
                    self.configuration.request_timeout_seconds,
                )
                if resp.status_code == 304 and cached is not None:
                    etag = resp.etag or cached.etag
                    now = datetime.now(timezone.utc)
                    self._memory = CachedModelCatalog(catalog=cached.catalog, fetched_at=now, etag=etag)
                    self._write_meta(etag, now)
                    return cached.catalog
                if not (200 <= resp.status_code <= 299):
                    raise UnexpectedStatusError(resp.status_code)
                cat = decode_catalog(resp.body)
                now = datetime.now(timezone.utc)
                self._memory = CachedModelCatalog(catalog=cat, fetched_at=now, etag=resp.etag)
                self._write_cache(resp.body, resp.etag, now)
                return cat
            except Exception as exc:
                if cached is not None:
                    return cached.catalog
                if isinstance(exc, ModelCatalogError):
                    raise
                raise UnavailableError(str(exc)) from exc

    async def load_async(self, force_refresh: bool = False) -> LingXiModelCatalog:
        return await asyncio.to_thread(self.load, force_refresh)

    def _write_cache(self, raw: bytes, etag: str | None, fetched_at: datetime) -> None:
        cache_dir = self._cache_dir()
        if cache_dir is None:
            return
        _write_atomic(cache_dir / "models.json", raw)
        self._write_meta(etag, fetched_at)

    def _write_meta(self, etag: str | None, fetched_at: datetime) -> None:
        cache_dir = self._cache_dir()
        if cache_dir is None:
            return
        payload = json.dumps(
            {"etag": etag, "fetchedAt": fetched_at.astimezone(timezone.utc).isoformat()}
        ).encode("utf-8")
        _write_atomic(cache_dir / "models.json.meta.json", payload)


def _write_atomic(target: Path, data: bytes) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(dir=target.parent, prefix=".cache-")
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
        os.replace(tmp_name, target)
    finally:
        if os.path.exists(tmp_name):
            os.remove(tmp_name)
