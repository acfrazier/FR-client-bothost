#!/usr/bin/env python3
"""Verify generated revision-377 metadata against Java, fixtures and the engine.

The Java source is required and supplies every opcode and length on each run.
--engine-root adds an independent cross-check against ClientGameProt.ts.
--cache-dir also checks real JAG inputs and byte-for-byte regeneration.
No product files are changed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path

from generate_revision_377_contract import products, protocol_contract

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/revision-377/protocol-377.json"
FIXTURES = ROOT / "crates/client/tests/fixtures/revision_377"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def verify(java: Path, engine: Path | None, cache: Path | None) -> str:
    contract = json.loads(CONTRACT.read_text())
    require(contract["revision"] == 377, "wrong contract revision")
    for name, digest in contract["source_pin"]["sha256"].items():
        require(hashlib.sha256((java / name).read_bytes()).hexdigest() == digest,
                f"Java source changed: {name}")
    require(contract == protocol_contract(java), "protocol contract differs from Java source")
    protocol = (java / "src/main/java/jagex2/io/Protocol.java").read_text()
    array = protocol.split("SERVERPROT_LENGTH = new int[] {", 1)[1].split("}", 1)[0]
    java_sizes = [int(value) for value in array.split(",")]
    inbound = contract["inbound"]
    require([row["id"] for row in inbound] == list(range(256)), "inbound IDs must cover every byte")
    require([row["length"] for row in inbound] == java_sizes[:256], "inbound sizes differ from Java")
    require(contract["unreachable_java_sizes"] == java_sizes[256:] == [0], "unreachable Java tail changed")
    rust_sizes = (ROOT / "crates/client/src/io/server_prot_sizes_377.inc.rs").read_text()
    rust_array = rust_sizes.split("= [", 1)[1].split("]", 1)[0]
    require([int(value) for value in rust_array.split(",") if value.strip()] == java_sizes[:256],
            "Rust inbound sizes differ from Java")

    client = (java / "src/main/java/jagex2/client/Client.java").read_text()
    java_ids = {int(value) for value in re.findall(r"this\.out\.p1isaac\((\d+)\)", client)}
    rows = contract["outbound"]
    by_id = {row["id"]: row for row in rows}
    require(len(by_id) == len(rows) and set(by_id) == java_ids, "outbound coverage differs from Java")
    require(len({row["name"] for row in rows}) == len(rows), "outbound names repeat")
    require(all(row["length"] >= -1 and row["sites"] for row in rows), "invalid outbound row")
    rust = (ROOT / "crates/client/src/io/client_prot_377.rs").read_text()
    constants = re.findall(
        r"pub const (\w+): ClientProt = ClientProt\s*\{\s*id: (\d+),\s*length: (-?\d+),?\s*\}", rust
    )
    require([(name, int(opcode), int(size)) for name, opcode, size in constants] ==
            [(row["name"], row["id"], row["length"]) for row in rows], "Rust outbound constants differ")
    all_names = re.findall(r"Self::([A-Z][A-Z0-9_]+),", rust)
    require(all_names == [row["name"] for row in rows], "ClientProt377::ALL differs")

    manifest = json.loads((FIXTURES / "manifest.json").read_text())
    require(manifest["revision"] == 377 and manifest["source_anchors"], "bad frame fixture provenance")
    names = set()
    for case in manifest["cases"]:
        require(case["name"] not in names, "duplicate frame fixture")
        names.add(case["name"])
        frame = bytes.fromhex(case["frame_hex"])
        require(frame[0] == case["opcode"], "fixture opcode mismatch")
        length = java_sizes[frame[0]]
        header = 1 if length >= 0 else 2 if length == -1 else 3
        require(header == case["header_length"], "fixture frame header mismatch")
        declared = length if length >= 0 else int.from_bytes(frame[1:header], "big")
        if case["payload_hex"] is None:
            require(len(frame) < header + declared, "pending fixture is complete")
        else:
            payload = bytes.fromhex(case["payload_hex"])
            require(len(payload) == declared and frame[header:] == payload, "complete fixture payload mismatch")
    cache_fixture = json.loads((FIXTURES / "cache.json").read_text())
    require(cache_fixture["revision"] == 377, "wrong cache revision")
    require([row["crc_slot"] for row in cache_fixture["archives"]] == list(range(1, 9)), "bad cache slots")
    require(sum(len(row["members"]) for row in cache_fixture["archives"]) > 0, "empty cache oracle")

    checks = ["Java-derived lengths and complete opcode coverage", "Rust tables", "5 framing fixtures"]
    if engine is not None:
        path = engine / "src/network/game/client/ClientGameProt.ts"
        table = {int(opcode): int(size) for opcode, size in
                 re.findall(r"new ClientGameProt\((\d+), (-?\d+)\)", path.read_text())}
        require(table == {row["id"]: row["length"] for row in rows}, "engine outbound table differs from Java-derived rows")
        checks.append(f"independent engine table ({len(table)} rows)")
    if cache is not None:
        for path, content in products(java, cache).items():
            require(path.read_text() == content, f"stale generated file: {path.relative_to(ROOT)}")
        checks.append("8 real JAGs and all member hashes, byte-identical regeneration")
    return f"PASS: 256 inbound, {len(rows)} outbound; " + "; ".join(checks)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java-root", type=Path, required=True)
    parser.add_argument("--engine-root", type=Path)
    parser.add_argument("--cache-dir", type=Path)
    args = parser.parse_args()
    try:
        print(verify(args.java_root, args.engine_root, args.cache_dir))
    except (ValueError, KeyError, IndexError, OSError) as error:
        raise SystemExit(f"FAIL: {error}") from error


if __name__ == "__main__":
    main()
