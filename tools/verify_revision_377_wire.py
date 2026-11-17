#!/usr/bin/env python3
"""Compile original Java Packet/Isaac and compare the synthetic wire fixtures.

Usage: python3 -B tools/verify_revision_377_wire.py --java-root JAVA --work-dir OUT
Requires JDK 17+. OUT holds compiler products, never the Java checkout.
This proves crypto bytes, wrapper layout and response text, not live behavior.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PIN = "327880f6de74b40c420705bc42e4b34284885087"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java-root", required=True, type=Path)
    parser.add_argument("--work-dir", required=True, type=Path)
    args = parser.parse_args()
    paths = ["jagex2/io/Packet.java", "jagex2/io/Isaac.java", "jagex2/client/Client.java",
             "jagex2/datastruct/DoublyLinkable.java", "jagex2/datastruct/Linkable.java",
             "jagex2/datastruct/LinkList.java", "deob/ObfuscatedName.java"]
    hashes = {}
    for relative in paths:
        name = "src/main/java/" + relative
        original = subprocess.check_output(["git", "-C", str(args.java_root), "show", f"{PIN}:{name}"])
        observed = (args.java_root / name).read_bytes()
        if observed != original:
            raise SystemExit(f"FAIL: Java source differs from pin: {relative}")
        hashes[relative] = hashlib.sha256(observed).hexdigest()
    args.work_dir.mkdir(parents=True, exist_ok=True)
    classes = args.work_dir / "classes"
    subprocess.run(["javac", "-d", str(classes), "-sourcepath", str(args.java_root / "src/main/java"),
                    str(ROOT / "tools/Revision377WireOracle.java")], check=True)
    raw = subprocess.check_output(["java", "-cp", str(classes), "Revision377WireOracle"], text=True)
    actual = json.loads(raw)
    actual["source_pin"] = PIN
    actual["messages"] = {}
    source = (args.java_root / "src/main/java/jagex2/client/Client.java").read_text()
    for code in (6, 18, 22, 23, 24, 25, 26):
        match = re.search(r'else if \(var8 == ' + str(code) + r'\) \{\s*'
                          r'this.loginMessage0 = "([^"]*)";\s*this.loginMessage1 = "([^"]*)";', source)
        if match is None:
            raise SystemExit(f"FAIL: Java message branch changed: {code}")
        actual["messages"][str(code)] = {"lines": source.count("\n", 0, match.start()) + 1,
                                         "text": [match[1], match[2]]}
    expected = json.loads((ROOT / "crates/client/tests/fixtures/revision_377/wire.json").read_text())
    (args.work_dir / "observed.json").write_text(json.dumps(actual, indent=2) + "\n")
    (args.work_dir / "source-hashes.json").write_text(json.dumps(hashes, indent=2) + "\n")
    if actual != expected:
        raise SystemExit("FAIL: wire fixture differs from original Java")
    print("PASS: original Java 2050 ISAAC words, 7 RSA cases, 2 wrappers, 7 response messages")


if __name__ == "__main__":
    main()
