#!/usr/bin/env python3
"""
Validate registry/registry.json against conformance/evidence/support-matrix.json
and generate docs/support-matrix.md + website/static/provider-status.json.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate & generate LingXi support matrix")
    parser.add_argument("--check", action="store_true", help="Verify existing generated files match")
    args = parser.parse_args()

    registry = json.loads((ROOT / "registry" / "registry.json").read_text(encoding="utf-8"))
    matrix = json.loads((ROOT / "conformance" / "evidence" / "support-matrix.json").read_text(encoding="utf-8"))

    reg_providers = registry["providers"]
    ev_providers = matrix["providers"]

    schema = json.loads((ROOT / "schemas/provider-registry-v1.schema.json").read_text(encoding="utf-8"))
    entry_properties = schema["properties"]["providers"]["additionalProperties"]["properties"]
    auth_types = set(entry_properties["auth"]["properties"]["type"]["enum"])
    counts = Counter(entry["verificationStatus"] for entry in reg_providers.values())
    summary = registry["summary"]
    expected = {"totalProviders": len(reg_providers), **{tier: counts[tier] for tier in matrix["tiers"]}}
    if summary != expected or sum(expected[tier] for tier in matrix["tiers"]) != len(reg_providers):
        raise SystemExit("ERROR: registry summary does not match entries")
    expected_evidence = {pid for pid, entry in reg_providers.items() if entry["verificationStatus"] in {"mock_tested", "live_verified", "full_support"}}
    if ev_providers.keys() != expected_evidence:
        raise SystemExit("ERROR: evidence providers do not match verified registry entries")

    status_map: dict[str, dict[str, object]] = {}
    for pid, entry in sorted(reg_providers.items()):
        status = entry["verificationStatus"]
        auth_type = entry["auth"]["type"]
        protocols = set(entry["protocols"])
        endpoints = entry["officialEndpoints"]
        if pid != entry["id"] or auth_type not in auth_types or not endpoints.keys() <= protocols:
            raise SystemExit(f"ERROR: invalid registry definition for {pid}")
        if status == "discovered":
            if protocols or auth_type != "unknown":
                raise SystemExit(f"ERROR: discovered provider {pid} declares runtime compatibility")
            continue
        for url in endpoints.values():
            parsed = urlsplit(url)
            local = parsed.hostname in {"127.0.0.1", "localhost", "::1"}
            if not parsed.hostname or parsed.username is not None or parsed.query or parsed.fragment or (
                parsed.scheme != "https" and not (local and parsed.scheme == "http")
            ):
                raise SystemExit(f"ERROR: invalid endpoint for {pid}")
        advertised = entry["protocols"]
        if pid in ev_providers:
            evidence = ev_providers[pid]
            verified = {item["protocol"] for item in entry["verificationEvidence"] if item["level"] == status}
            if evidence["status"] != status or not verified or set(evidence["protocols"]) != verified or not verified <= protocols:
                raise SystemExit(f"ERROR: inconsistent protocol evidence for {pid}")
            if not evidence["evidence"] or not all((ROOT / item["suite"]).exists() for item in evidence["evidence"]):
                raise SystemExit(f"ERROR: missing test evidence for {pid}")
            advertised = evidence["protocols"]
        status_map[pid] = {
            "name": entry["name"], "status": status, "protocols": advertised,
            "endpoints": sorted(set(endpoints.values())),
        }

    website_status_bytes = (
        json.dumps(status_map, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")

    lines = [
        "# Provider & Protocol Support Matrix",
        "",
        "Generated from `registry/registry.json` and `conformance/evidence/support-matrix.json`.",
        "",
        "## Summary",
        "",
        f"- Total providers in registry: **{summary['totalProviders']}**",
        f"- `full_support`: **{summary['full_support']}**",
        f"- `live_verified`: **{summary['live_verified']}**",
        f"- `mock_tested`: **{summary['mock_tested']}**",
        f"- `compatible` (declarative OpenAI-compatible, awaiting mock/live verification): **{summary['compatible']}**",
        f"- `discovered` (catalog metadata only, zero inferred protocol claims): **{summary['discovered']}**",
        "",
        "## Verified Providers (`mock_tested`)",
        "",
        "| Provider | Status | Protocols | Evidence |",
        "| --- | --- | --- | --- |",
    ]
    for pid, ev in sorted(ev_providers.items()):
        protos = ", ".join(f"`{p}`" for p in ev["protocols"])
        suites = ", ".join(f"`{e['suite']}` ({e['verifiedAt']})" for e in ev["evidence"])
        lines.append(f"| `{pid}` | `{ev['status']}` | {protos} | {suites} |")

    lines.extend(["", "## Declarative Compatible Providers (`compatible`)", ""])
    compat_ids = [pid for pid, e in sorted(reg_providers.items()) if e["verificationStatus"] == "compatible"]
    lines.append(", ".join(f"`{pid}`" for pid in compat_ids))
    lines.append("")
    md_bytes = ("\n".join(lines)).encode("utf-8")

    status_path = ROOT / "website" / "static" / "provider-status.json"
    md_path = ROOT / "docs" / "support-matrix.md"

    if args.check:
        if not status_path.exists() or status_path.read_bytes() != website_status_bytes:
            raise SystemExit("ERROR: website/static/provider-status.json is out of date")
        if not md_path.exists() or md_path.read_bytes() != md_bytes:
            raise SystemExit("ERROR: docs/support-matrix.md is out of date")
        print(f"Support matrix verified: {summary['totalProviders']} providers ({summary['mock_tested']} mock_tested, {summary['compatible']} compatible, {summary['discovered']} discovered).")
        return 0

    status_path.write_bytes(website_status_bytes)
    md_path.parent.mkdir(parents=True, exist_ok=True)
    md_path.write_bytes(md_bytes)
    print(f"Generated {status_path} and {md_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
