#!/usr/bin/env python3
"""Compile untouched Client-Java 377 and freeze decoded field digests.

The Java helper calls the original decoders, not a Python translation or Rust
round trip. No Java checkout, cache archive or existing shared file is written.
Floor field807 alone is excluded because it includes Math.random() jitter.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "crates/client/tests/fixtures/revision_377/decoders.json"
TABLES = ("seq", "loc", "flo", "obj", "npc", "idk", "spotanim", "varp", "varbit", "interface")


def canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def summarize(decoded: dict) -> dict:
    tables = {}
    for name in TABLES:
        rows = decoded[name]
        if name == "flo":
            for row in rows:
                del row["field807"]
        fields = sorted(next(row for row in rows if row is not None))
        samples = {0, len(rows) // 2, len(rows) - 1}
        # Include the first distinct value of each field, not just arbitrary IDs.
        for field in fields:
            first = next(row[field] for row in rows if row is not None)
            for index, row in enumerate(rows):
                if row is not None and row[field] != first:
                    samples.add(index)
                    break
        tables[name] = {
            "count": len(rows),
            "present": sum(row is not None for row in rows),
            "fields": {field: digest(canonical([None if row is None else row[field] for row in rows])) for field in fields},
            "samples": {str(index): rows[index] for index in sorted(samples)},
        }
    return {
        "tables": tables,
        "tracked_varps": decoded["tracked_varps"],
        "interface_layers": decoded["interface_layers"],
        "interface_records": {
            "count": len(decoded["interface_records"]),
            "sha256": digest(canonical(decoded["interface_records"])),
        },
        "obj_members": decoded["obj_members"],
        "obj_nonmembers": decoded["obj_nonmembers"],
        "excluded_nondeterministic_field": "FloType.field807 (Math.random jitter)",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java-root", type=Path, required=True)
    parser.add_argument("--cache-dir", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    args.work_dir.mkdir(parents=True, exist_ok=True)
    sources = sorted((args.java_root / "src/main/java").rglob("*.java"))
    if not sources:
        raise ValueError("Java source directory is empty")
    manifest = json.loads((FIXTURE.parent / "cache.json").read_text())
    cache_inputs = {}
    for row in manifest["archives"]:
        if row["name"] not in ("config", "interface"):
            continue
        data = (args.cache_dir / row["name"]).read_bytes()
        if len(data) != row["length"] or digest(data) != row["sha256"]:
            raise ValueError(f"wrong real 377 cache: {row['name']}")
        cache_inputs[row["name"]] = {"length": len(data), "sha256": digest(data)}
    source_inputs = {str(path.relative_to(args.java_root)): digest(path.read_bytes()) for path in sources}
    classes = args.work_dir / "classes"
    classes.mkdir(exist_ok=True)
    subprocess.run(["javac", "-encoding", "UTF-8", "-d", str(classes), *map(str, sources),
                    str(ROOT / "tools/Revision377DecoderOracle.java")], check=True)
    dump = args.work_dir / "java-decoded.json"
    subprocess.run(["java", "-Djava.awt.headless=true", "-cp", str(classes),
                    "Revision377DecoderOracle", str(args.cache_dir), str(dump)], check=True)
    result = {
        "revision": 377,
        "oracle": "untouched Client-Java 377 decoders, reflected primitive/string/array fields",
        "java_sources": source_inputs,
        "cache_inputs": cache_inputs,
        **summarize(json.loads(dump.read_text())),
    }
    output = json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2) + "\n"
    if args.check:
        if FIXTURE.read_text() != output:
            raise ValueError("stale Java-derived decoder oracle")
    else:
        FIXTURE.write_text(output)
    count = sum(table["present"] for table in result["tables"].values())
    print(f"PASS: {'checked' if args.check else 'generated'} {len(TABLES)} tables, {count} decoded records, all deterministic Java fields")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
