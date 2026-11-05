#!/usr/bin/env python3
"""Freeze Java 377 sound occupancy, optimized delays, loops and tone timings.

No PCM parity is claimed: Java's noise generator uses Math.random().
The oracle executes the original loader without rewriting the Java source.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "crates/client/tests/fixtures/revision_377/sounds.json"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java-root", type=Path, required=True)
    parser.add_argument("--cache-dir", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    args.work_dir.mkdir(parents=True, exist_ok=True)
    classes = args.work_dir / "classes"
    classes.mkdir(exist_ok=True)
    sources = sorted((args.java_root / "src/main/java").rglob("*.java"))
    if not sources:
        raise ValueError("Java source directory is empty")
    subprocess.run(["javac", "-encoding", "UTF-8", "-d", str(classes), *map(str, sources),
                    str(ROOT / "tools/Revision377DecoderOracle.java"),
                    str(ROOT / "tools/Revision377SoundOracle.java")], check=True)
    dump = args.work_dir / "java-sounds.json"
    archive = args.cache_dir / "sounds"
    subprocess.run(["java", "-Djava.awt.headless=true", "-cp", str(classes),
                    "Revision377SoundOracle", str(archive), str(dump)], check=True)
    consumed, rows = json.loads(dump.read_text())
    high_ids = [i for i, row in enumerate(rows) if i > 999 and row is not None]
    assert 1278 in high_ids
    samples = sorted({0, 1278, high_ids[0], high_ids[len(high_ids) // 2], high_ids[-1]})
    result = {
        "revision": 377,
        "oracle": "untouched Client-Java 377 Wave.unpack/method481",
        "projection": "null or [delay, loop_begin, loop_end, ten null-or-[length,start] tones]",
        "java_sources": {str(p.relative_to(args.java_root)): digest(p.read_bytes()) for p in sources},
        "sounds_sha256": digest(archive.read_bytes()),
        "decoded_bytes": consumed,
        "capacity": len(rows),
        "present": sum(row is not None for row in rows),
        "high_ids": len(high_ids),
        "rows_sha256": digest(json.dumps(rows, separators=(",", ":")).encode()),
        "samples": {str(i): rows[i] for i in samples},
    }
    output = json.dumps(result, sort_keys=True, indent=2) + "\n"
    if args.check:
        assert FIXTURE.read_text() == output, "stale Java 377 sound fixture"
    else:
        FIXTURE.write_text(output)
    print(f"PASS: {result['present']} sounds, {len(high_ids)} above 999, samples {samples}")


if __name__ == "__main__":
    main()
