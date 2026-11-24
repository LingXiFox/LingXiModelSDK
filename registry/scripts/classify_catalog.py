#!/usr/bin/env python3
"""
Merge curated declarative provider definitions with a catalog snapshot.

Strict rule: upstream `npm` package names are NEVER used to infer protocol
compatibility. Non-curated providers are recorded strictly as `discovered` with
empty `protocols` and upstream hints separated under `discoveryHints`.
"""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any

TIERS = ("discovered", "compatible", "mock_tested", "live_verified", "full_support")


def load_curated(curated_dir: Path) -> dict[str, dict[str, Any]]:
    entries: dict[str, dict[str, Any]] = {}
    for path in sorted(curated_dir.glob("*.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        pid = data["id"]
        entries[pid] = data
    return entries


def build_registry(
    curated_dir: Path,
    catalog_path: Path | None,
    generated_at: str | None = None,
) -> dict[str, Any]:
    curated = load_curated(curated_dir)
    catalog_providers: dict[str, Any] = {}
    if catalog_path is not None:
        raw = json.loads(catalog_path.read_text(encoding="utf-8"))
        if isinstance(raw, dict) and isinstance(raw.get("providers"), dict):
            catalog_providers = raw["providers"]
        elif isinstance(raw, dict):
            catalog_providers = raw

    merged: dict[str, dict[str, Any]] = {}
    all_ids = sorted(set(curated.keys()) | set(catalog_providers.keys()))
    for pid in all_ids:
        cat_entry = catalog_providers.get(pid) if isinstance(catalog_providers.get(pid), dict) else {}
        hints: dict[str, Any] = {}
        if isinstance(cat_entry.get("api"), str) and cat_entry["api"]:
            hints["upstreamApi"] = cat_entry["api"]
        if isinstance(cat_entry.get("npm"), str) and cat_entry["npm"]:
            hints["upstreamNpm"] = cat_entry["npm"]
        if isinstance(cat_entry.get("doc"), str) and cat_entry["doc"]:
            hints["doc"] = cat_entry["doc"]

        if pid in curated:
            entry = dict(curated[pid])
            if hints:
                entry["discoveryHints"] = hints
            merged[pid] = entry
            continue

        env = cat_entry.get("env") if isinstance(cat_entry.get("env"), list) else []
        env_strs = [e for e in env if isinstance(e, str)]
        merged[pid] = {
            "id": pid,
            "name": cat_entry.get("name") if isinstance(cat_entry.get("name"), str) and cat_entry["name"] else pid,
            "protocols": [],
            "officialEndpoints": {},
            "auth": {"type": "bearer" if env_strs else "none", "env": env_strs},
            "defaultHeaders": {},
            "extraParams": {},
            "thinking": {"mode": "none"},
            "modelOverrides": {},
            "verificationStatus": "discovered",
            "verificationEvidence": [],
            **({"discoveryHints": hints} if hints else {}),
        }

    counts = {tier: 0 for tier in TIERS}
    for entry in merged.values():
        status = entry["verificationStatus"]
        if status not in counts:
            raise ValueError(f"Invalid verificationStatus {status!r} on {entry['id']}")
        counts[status] += 1

    return {
        "schemaVersion": "1.0",
        "generatedAt": generated_at or time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "summary": {
            "totalProviders": len(merged),
            **counts,
        },
        "providers": merged,
    }


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description="Build LingXi declarative provider registry")
    parser.add_argument("--curated-dir", type=Path, default=root / "curated")
    parser.add_argument("--catalog", type=Path, default=None, help="Path to models.json or upstream api.json")
    parser.add_argument("--output", type=Path, default=root / "registry.json")
    args = parser.parse_args()

    doc = build_registry(args.curated_dir, args.catalog)
    args.output.write_text(json.dumps(doc, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(doc["summary"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
