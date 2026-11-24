#!/usr/bin/env python3
"""
Validate registry/registry.json against conformance/evidence/support-matrix.json
and generate docs/support-matrix.md + website/static/provider-status.json.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate & generate LingXi support matrix")
    parser.add_argument("--check", action="store_true", help="Verify existing generated files match")
    args = parser.parse_args()

    registry = json.loads((ROOT / "registry" / "registry.json").read_text(encoding="utf-8"))
    matrix = json.loads((ROOT / "conformance" / "evidence" / "support-matrix.json").read_text(encoding="utf-8"))

    reg_providers = registry["providers"]
    ev_providers = matrix["providers"]

    for pid, entry in reg_providers.items():
        status = entry["verificationStatus"]
        if status == "discovered" and entry["protocols"]:
            raise SystemExit(f"ERROR: discovered provider {pid} declares protocols")
        if status in ("mock_tested", "live_verified", "full_support"):
            if pid not in ev_providers or ev_providers[pid]["status"] != status:
                raise SystemExit(f"ERROR: {pid} has status {status} in registry without matching support-matrix evidence")
            if not ev_providers[pid].get("evidence"):
                raise SystemExit(f"ERROR: {pid} missing evidence array in support-matrix.json")

    summary = registry["summary"]
    status_map: dict[str, dict[str, object]] = {}
    for pid, entry in sorted(reg_providers.items()):
        if entry["verificationStatus"] != "discovered":
            status_map[pid] = {
                "status": entry["verificationStatus"],
                "protocols": entry["protocols"],
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
