#!/usr/bin/env python3
"""Run 377 extraction and Java-only drift regressions.

Usage: python3 -B tools/test_revision_377_contract.py --java-root CLIENT_JAVA
Temporary files follow TMPDIR. No cache or engine checkout is needed.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import tempfile
import unittest
from unittest.mock import patch

from generate_revision_377_contract import outbound_rows
import verify_revision_377_contract as verifier


def java_method(body: str) -> str:
    return "public class Client { public void emit() {\n" + body + "\n} }\n"


class OutboundExtractionTests(unittest.TestCase):
    def test_fixed_packet_in_emitting_branch(self) -> None:
        rows = outbound_rows(java_method("""
            if (selected) {
                // OPNPC1
                this.out.p1isaac(112);
                this.out.p2_alt1(value);
            }
        """))
        self.assertEqual([(row["id"], row["length"]) for row in rows], [(112, 2)])

    def test_bare_opcode_block_cannot_hide_a_fixed_write(self) -> None:
        with self.assertRaisesRegex(ValueError, "unsupported opcode block"):
            outbound_rows(java_method("""
                if (selected) {
                    {
                        // OPNPC1
                        this.out.p1isaac(112);
                    }
                    this.out.p2_alt1(value);
                }
            """))

    def test_fixed_write_after_emitting_branch_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "packet write outside opcode block"):
            outbound_rows(java_method("""
                if (selected) {
                    // OPNPC1
                    this.out.p1isaac(112);
                }
                this.out.p2_alt1(value);
            """))

    def test_conditional_fixed_write_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "conditional/dynamic fixed write"):
            outbound_rows(java_method("""
                // OPNPC1
                this.out.p1isaac(112);
                if (selected) {
                    this.out.p2_alt1(value);
                }
            """))

    def test_literal_loops_keep_their_fixed_width(self) -> None:
        rows = outbound_rows(java_method("""
            // IDK_SAVEDESIGN
            this.out.p1isaac(163);
            this.out.p1(gender);
            for (int i = 0; i < 7; i++) {
                this.out.p1(kits[i]);
            }
            for (int i = 0; i < 5; i++) {
                this.out.p1(colours[i]);
            }
        """))
        self.assertEqual(rows[0]["length"], 13)

    def test_separate_fixed_branches_do_not_share_writes(self) -> None:
        rows = outbound_rows(java_method("""
            if (selected) {
                // OPNPC1
                this.out.p1isaac(112);
                this.out.p2_alt1(value);
            } else {
                // NO_TIMEOUT
                this.out.p1isaac(40);
            }
        """))
        self.assertEqual([(row["id"], row["length"]) for row in rows], [(40, 0), (112, 2)])

    def test_variable_movement_can_have_a_shared_suffix(self) -> None:
        rows = outbound_rows(java_method("""
            if (mode == 0) {
                // MOVE_GAMECLICK
                this.out.p1isaac(28);
                this.out.p1(count + count + 3);
            }
            if (mode == 1) {
                // MOVE_MINIMAPCLICK
                this.out.p1isaac(213);
                this.out.p1(count + count + 3 + 14);
            }
            this.out.p2_alt3(x);
        """))
        self.assertEqual([row["length"] for row in rows], [-1, -1])


class JavaOnlyVerificationTests(unittest.TestCase):
    java: Path

    def test_real_java_without_cache_or_engine(self) -> None:
        self.assertTrue(verifier.verify(self.java, None, None).startswith("PASS:"))

    def test_matching_rust_and_json_length_drift_is_rejected(self) -> None:
        contract = json.loads(verifier.CONTRACT.read_text())
        row = next(row for row in contract["outbound"] if row["name"] == "OPNPC1")
        self.assertEqual(row["length"], 2)
        row["length"] = 3
        rust_relative = Path("crates/client/src/io/client_prot_377.rs")
        rust = (verifier.ROOT / rust_relative).read_text()
        mutated, count = re.subn(
            r"(pub const OPNPC1: ClientProt = ClientProt\s*\{\s*id: 112,\s*length: )2",
            r"\g<1>3", rust,
        )
        self.assertEqual(count, 1)
        with tempfile.TemporaryDirectory(prefix="revision-377-contract-") as directory:
            root = Path(directory)
            contract_path = root / "docs/revision-377/protocol-377.json"
            contract_path.parent.mkdir(parents=True)
            contract_path.write_text(json.dumps(contract))
            (root / rust_relative).parent.mkdir(parents=True)
            (root / rust_relative).write_text(mutated)
            inbound = Path("crates/client/src/io/server_prot_sizes_377.inc.rs")
            (root / inbound).write_text((verifier.ROOT / inbound).read_text())
            with patch.multiple(verifier, ROOT=root, CONTRACT=contract_path):
                with self.assertRaisesRegex(ValueError, "protocol contract differs from Java source"):
                    verifier.verify(self.java, None, None)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java-root", type=Path, required=True)
    args = parser.parse_args()
    JavaOnlyVerificationTests.java = args.java_root
    unittest.main(argv=[__file__], verbosity=2)
