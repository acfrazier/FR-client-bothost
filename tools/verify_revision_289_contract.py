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
    inbound_by_id = {row["id"]: row for row in contract["inbound"]}
    cases = manifest.get("cases")
    if not isinstance(cases, list) or not cases:
        fail("fixture cases must be a non-empty array")
    names = set()
    for case in cases:
        for key in ("name", "opcode", "length_kind", "frame_hex", "payload_hex", "source_anchors", "expected_consumed_length", "expected_cursor", "expected_result", "oracle_derivation"):
            if key not in case:
                fail(f"fixture missing {key}: {case!r}")
        if case["name"] in names:
            fail(f"duplicate fixture name {case['name']}")
        names.add(case["name"])
        if case["opcode"] not in inbound_by_id:
            fail(f"fixture opcode is not in inbound contract: {case['name']}")
        frame = re.sub(r"\s+", "", case["frame_hex"])
        payload = re.sub(r"\s+", "", case["payload_hex"])
        if not re.fullmatch(r"[0-9a-fA-F]*", frame) or len(frame) % 2:
            fail(f"fixture frame_hex is not byte-aligned hex: {case['name']}")
        if not re.fullmatch(r"[0-9a-fA-F]*", payload) or len(payload) % 2:
            fail(f"fixture payload_hex is not byte-aligned hex: {case['name']}")
        if int(frame[:2], 16) != case["opcode"]:
            fail(f"fixture opcode/header mismatch: {case['name']}")
        row_length = inbound_by_id[case["opcode"]]["length"]
        expected_kind = { -1: "g1", -2: "g2" }.get(row_length, "fixed")
        if case["length_kind"] != expected_kind:
            fail(f"fixture length kind disagrees with protocol row: {case['name']}")
        header_size = {"fixed": 1, "g1": 2, "g2": 3}[case["length_kind"]]
        if case["length_kind"] == "fixed":
            declared = row_length
            if declared != len(payload) // 2:
                fail(f"fixed fixture payload length mismatch: {case['name']}")
        else:
            if "declared_payload_length" not in case:
                fail(f"variable fixture missing declared_payload_length: {case['name']}")
            declared = case["declared_payload_length"]
            if declared != int(frame[2:2 + (2 if case["length_kind"] == "g1" else 4)], 16):
                fail(f"declared length bytes mismatch: {case['name']}")
            if declared != len(payload) // 2 and case["expected_result"]["status"] == "dispatch":
                fail(f"complete variable fixture payload length mismatch: {case['name']}")
        expected_status = case["expected_result"]["status"]
        expected_consumed = header_size if expected_status == "pending" else header_size + declared
        if case["expected_consumed_length"] != expected_consumed:
            fail(f"expected consumed length disagrees with source cursor rule: {case['name']}")
        if len(frame) // 2 < case["expected_consumed_length"]:
            fail(f"fixture frame is shorter than consumed prefix: {case['name']}")
        if not isinstance(case["expected_result"], dict) or "status" not in case["expected_result"]:
            fail(f"fixture expected_result must be structured: {case['name']}")
        if any(word in case["oracle_derivation"].lower() for word in ("new rust", "proposed rust", "placeholder", "depends on selected table", "hand-waved")):
            fail(f"fixture oracle must be independent: {case['name']}")
        if not case["source_anchors"]:
            fail(f"fixture source_anchors empty: {case['name']}")
    print(f"PASS: {len(contract['inbound'])} inbound, {len(contract['outbound'])} outbound rows; {len(cases)} fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
