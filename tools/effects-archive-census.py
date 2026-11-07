#!/usr/bin/env python3
"""Census GFX roots and ECM -> GFX references from retail Perfect World PCKs.

This deliberately writes metadata only: names, sizes, signatures, short decoded
resource paths, and dependency edges. It never copies client assets into the
repository or the output directory.
"""
from __future__ import annotations

import argparse
import collections
import csv
import json
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import pck_lib  # noqa: E402


PATH_RE = re.compile(
    r"(?i)([\w\u4e00-\u9fff .\\/\-]{2,180}\.(?:gfx|dds|mox|ecm|ski|bon|smd))"
)


def ext_of(name: str) -> str:
    return name.rsplit(".", 1)[-1].lower() if "." in name else ""


def extract_resource_paths(blob: bytes) -> list[str]:
    # Client paths are commonly GBK-encoded; tolerate mixed/binary regions and
    # filter candidates to known resource extensions.
    text = blob.decode("gbk", errors="ignore")
    found: set[str] = set()
    for match in PATH_RE.finditer(text):
        candidate = match.group(1).replace("\\", "/").strip()
        candidate = re.sub(r"\s+", " ", candidate)
        if candidate and not candidate.startswith("."):
            found.add(candidate)
    return sorted(found, key=str.casefold)


def ascii_strings(blob: bytes, minimum: int = 5, limit: int = 24) -> list[str]:
    values = re.findall(rb"[\x20-\x7e]{%d,}" % minimum, blob)
    return [v[:180].decode("ascii", "replace") for v in values[:limit]]


def census_pack(pack_path: pathlib.Path, out_dir: pathlib.Path) -> dict:
    version, entries = pck_lib.directory(str(pack_path))
    ext_counts = collections.Counter(ext_of(row[0]) for row in entries)
    summary = {
        "archive": pack_path.name,
        "version": version,
        "member_count": len(entries),
        "extension_counts": dict(sorted(ext_counts.items())),
    }

    if pack_path.name.lower() == "gfx.pck":
        roots_path = out_dir / "gfx-roots.csv"
        with roots_path.open("w", encoding="utf-8-sig", newline="") as f:
            writer = csv.DictWriter(
                f, fieldnames=["name", "size", "stored_size", "header_hex",
                               "ascii_strings", "resource_refs"]
            )
            writer.writeheader()
            for entry in entries:
                name, _offset, size, stored_size = entry
                if ext_of(name) != "gfx":
                    continue
                blob = pck_lib.read_entry(str(pack_path), entry)
                writer.writerow({
                    "name": name,
                    "size": size,
                    "stored_size": stored_size,
                    "header_hex": blob[:96].hex(),
                    "ascii_strings": json.dumps(ascii_strings(blob)),
                    "resource_refs": json.dumps(extract_resource_paths(blob)),
                })
        summary["gfx_root_count"] = sum(1 for e in entries if ext_of(e[0]) == "gfx")
        summary["gfx_roots_csv"] = roots_path.name

    if pack_path.name.lower() in ("models.pck", "litmodels.pck"):
        refs_path = out_dir / (pack_path.stem.lower() + "-ecm-gfx-references.csv")
        with refs_path.open("w", encoding="utf-8-sig", newline="") as f:
            writer = csv.DictWriter(f, fieldnames=["model_pack", "ecm_model", "resource_path"])
            writer.writeheader()
            edge_count = 0
            for entry in entries:
                name = entry[0]
                if ext_of(name) != "ecm":
                    continue
                blob = pck_lib.read_entry(str(pack_path), entry)
                for resource in extract_resource_paths(blob):
                    if resource.lower().endswith(".gfx"):
                        writer.writerow({
                            "model_pack": pack_path.name,
                            "ecm_model": name,
                            "resource_path": resource,
                        })
                        edge_count += 1
        summary["ecm_gfx_reference_count"] = edge_count
        summary["ecm_gfx_references_csv"] = refs_path.name

    return summary


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--client-dir", required=True, type=pathlib.Path)
    ap.add_argument("--out-dir", required=True, type=pathlib.Path)
    args = ap.parse_args()
    args.out_dir.mkdir(parents=True, exist_ok=True)

    reports = []
    for name in ("gfx.pck", "models.pck", "litmodels.pck", "textures.pck",
                 "grasses.pck", "facedata.pck"):
        path = args.client_dir / name
        if not path.is_file():
            continue
        try:
            reports.append(census_pack(path, args.out_dir))
        except Exception as exc:  # retain per-archive failure details for diagnosis
            reports.append({"archive": name, "error": f"{type(exc).__name__}: {exc}"})

    output = args.out_dir / "pck-summary.json"
    output.write_text(json.dumps(reports, indent=2, ensure_ascii=False) + "\n",
                      encoding="utf-8")
    for report in reports:
        if "error" in report:
            print(f"{report['archive']}: ERROR {report['error']}")
        else:
            print(f"{report['archive']}: version={report['version']:#x}, "
                  f"members={report['member_count']}, "
                  f"gfx_roots={report.get('gfx_root_count', 0)}, "
                  f"ecm_gfx_refs={report.get('ecm_gfx_reference_count', 0)}")
    print(f"summary: {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
