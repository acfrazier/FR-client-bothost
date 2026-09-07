#!/usr/bin/env python3
"""Validate the public revision-289 contract and independently derived fixtures."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/revision-289/protocol-289.json"
FIXTURES = ROOT / "crates/client/tests/fixtures/revision_289/manifest.json"


def fail(message: str) -> None:
    raise SystemExit(f"FAIL: {message}")


def main() -> int:
    contract = json.loads(CONTRACT.read_text())
    if contract.get("revision") != 289:
        fail("contract revision must be 289")
    if not isinstance(contract.get("source_pin"), dict):
        fail("source_pin must be an object")
    for direction in ("inbound", "outbound"):
        rows = contract.get(direction)
        if not isinstance(rows, list) or not rows:
            fail(f"{direction} must be a non-empty array")
        ids = []
        for row in rows:
            required = ("name", "id", "length", "fields", "source_anchors", "confidence")
            missing = [key for key in required if key not in row]
            if missing:
                fail(f"{direction} row missing {missing}: {row!r}")
            if not isinstance(row["id"], int) or not 0 <= row["id"] <= 255:
                fail(f"{direction} id out of byte range: {row!r}")
            if row["id"] in ids:
                fail(f"duplicate {direction} id {row['id']}")
            ids.append(row["id"])
            if not isinstance(row["fields"], list) or not row["fields"]:
                fail(f"{direction} fields must be a non-empty ordered list")
            if not isinstance(row["source_anchors"], list) or not row["source_anchors"]:
                fail(f"{direction} source_anchors must be non-empty")
        if direction == "inbound" and set(ids) != set(range(256)):
            fail("inbound must enumerate all 256 decoded opcode IDs")
    manifest = json.loads(FIXTURES.read_text())
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        fail("fixture cases must be a non-empty array")
    names = set()
    for case in cases:
        for key in ("name", "hex", "source_anchors", "expected_consumed_length", "expected_decoded_state", "oracle_derivation"):
            if key not in case:
                fail(f"fixture missing {key}: {case!r}")
        if case["name"] in names:
            fail(f"duplicate fixture name {case['name']}")
        names.add(case["name"])
        raw = re.sub(r"\s+", "", case["hex"])
        if not re.fullmatch(r"[0-9a-fA-F]*", raw) or len(raw) % 2:
            fail(f"fixture hex is not byte-aligned hex: {case['name']}")
        if len(raw) // 2 != case["expected_consumed_length"]:
            fail(f"fixture length mismatch: {case['name']}")
        if "new Rust" in case["oracle_derivation"] or "proposed Rust" in case["oracle_derivation"]:
            fail(f"fixture oracle must be independent: {case['name']}")
        if not case["source_anchors"]:
            fail(f"fixture source_anchors empty: {case['name']}")
    print(f"PASS: {len(contract['inbound'])} inbound, {len(contract['outbound'])} outbound rows; {len(cases)} fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
