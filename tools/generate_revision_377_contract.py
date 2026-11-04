#!/usr/bin/env python3
"""Extract revision-377 packet tables and cache oracles from the Java client.

Usage: python3 tools/generate_revision_377_contract.py --java-root CLIENT_JAVA \
           --cache-dir EXTRACTED_JAGS [--check]
The cache directory must contain all eight real JAGs, not a client JAR.
No Java or cache bytes are copied into the generated, public-safe fixtures.
"""
from __future__ import annotations

import argparse
import bz2
import hashlib
import json
import re
import subprocess
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE_COMMIT = "327880f6de74b40c420705bc42e4b34284885087"
CLIENT = "src/main/java/jagex2/client/Client.java"
PROTOCOL = "src/main/java/jagex2/io/Protocol.java"
JAGFILE = "src/main/java/jagex2/io/Jagfile.java"
ONDEMAND = "src/main/java/jagex2/io/OnDemand.java"
SOURCE_HASHES = {
    CLIENT: "09f77c9540f2a2fa85cd9c4f158b5eff70baf0ae9d1c509478f53f6546f4cf6c",
    PROTOCOL: "6e7d6b49309217e120aaf69be23a5662ca89097168754e9ac67697695f6da52e",
    JAGFILE: "ca4dd7783f522ac9a69b975505a8bab650b7fcb45a9c17dc6ade22637cbb9f07",
    ONDEMAND: "6dede5b2e9fa6a7ed71a77bdb1718b2465418e625a44803ae753d6faeab99c69",
}
NAMES = ("title", "config", "interface", "media", "versionlist", "textures", "wordenc", "sounds")
# These normalize Java comments to the existing ClientProt/engine vocabulary.
COMMENT_NAMES = {
    "TUTORIAL_CLICKSIDE": "TUT_CLICKSIDE",
    "IF_PLAYERDESIGN": "IDK_SAVEDESIGN",
    "REPORT_ABUSE": "SEND_SNAPSHOT",
    "todo: camera position": "EVENT_CAMERA_POSITION",
    "todo: applet focus": "EVENT_APPLET_FOCUS",
    "todo: notifying the client is in a frame": "EVENT_HAS_WINDOW",
    "todo: reporting synth error": "EVENT_SYNTH_ERROR",
}


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def format_rust(text: str) -> str:
    # rustfmt uses this repository's rust-toolchain.toml pin, not a width heuristic.
    return subprocess.run(
        ["rustfmt", "--edition", "2021", "--emit", "stdout"],
        input=text, text=True, capture_output=True, check=True, cwd=ROOT,
    ).stdout


def anchor(source: str, text: str, start: int, end: int) -> str:
    first = text.count("\n", 0, start) + 1
    last = text.count("\n", 0, end) + 1
    return f"{source}:{first}-{last}"


def code_only(text: str) -> str:
    # Preserve byte positions and newlines while hiding comments and literals.
    return re.sub(
        r'//[^\n]*|/\*[\s\S]*?\*/|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'',
        lambda match: re.sub(r"[^\n]", " ", match.group()),
        text,
    )


def pairs(code: str, opening: str, closing: str) -> dict[int, int]:
    stack = []
    result = {}
    for pos, char in enumerate(code):
        if char == opening:
            stack.append(pos)
        elif char == closing:
            if not stack:
                raise ValueError(f"unbalanced {opening}{closing}")
            result[stack.pop()] = pos
    if stack:
        raise ValueError(f"unbalanced {opening}{closing}")
    return result


def enclosing(blocks: dict[int, int], pos: int) -> tuple[int, int]:
    return max((start, end) for start, end in blocks.items() if start < pos < end)


def outbound_rows(text: str) -> list[dict]:
    code = code_only(text)
    blocks = pairs(code, "{", "}")
    parens = pairs(code, "(", ")")
    calls = list(re.finditer(r"this\.out\.p1isaac\((\d+)\)", code))
    rows = {}
    for call in calls:
        opcode = int(call[1])
        prefix = text[:call.start()].rstrip()
        comment = prefix[prefix.rfind("\n") + 1:].strip().removeprefix("//").strip()
        name = COMMENT_NAMES.get(comment, comment)
        if not re.fullmatch(r"[A-Z][A-Z0-9_]+", name):
            raise ValueError(f"unnamed outbound opcode {opcode}: {comment!r}")
        _, end = enclosing(blocks, call.start())
        next_call = re.search(r"this\.out\.p1isaac\(", code[call.end():end])
        if next_call:
            end = call.end() + next_call.start()
        body = code[call.end():end]
        writers = []
        for writer in re.finditer(r"this\.out\.(\w+)\(", body):
            start = call.end() + writer.start()
            close = parens[call.end() + writer.end() - 1]
            writers.append((writer[1], start, close + 1))
        first_argument = ""
        if writers:
            first_argument = text[writers[0][1]:writers[0][2]].split("(", 1)[1][:-1].strip()
        if any(writer == "psize1" for writer, _, _ in writers):
            length = -1
            derivation = "p1 length placeholder followed by psize1 backpatch"
        elif writers and writers[0][0] == "p1" and re.fullmatch(
            r"(?:this\.\w+\.length\(\) - \d+|(\w+) \+ \1 \+ \d+(?: \+ \d+)?)",
            first_argument,
        ):
            # Walk/cheat write a length expression, not an ordinary p1 field.
            length = -1
            derivation = "explicit p1 payload-length expression"
        else:
            length = 0
            for writer, start, _ in writers:
                width = re.fullmatch(r"p([12348])(?:_alt[123])?", writer)
                if width is None:
                    raise ValueError(f"unsupported fixed writer {writer} in {name}")
                multiplier = 1
                for begin, finish in blocks.items():
                    if call.end() < begin < start < finish:
                        # Fixed packets may contain only statically bounded loops.
                        header = code[call.end():begin].rsplit("}", 1)[-1]
                        loop = re.search(
                            r"for\s*\(int (\w+) = 0; \1 < (\d+); \1\+\+\)\s*$", header
                        )
                        if loop is None:
                            raise ValueError(f"conditional/dynamic fixed write in {name}")
                        multiplier *= int(loop[2])
                length += int(width[1]) * multiplier
            derivation = "sum of pN writer widths, expanding literal-count for loops"
        fields = [text[start:end].strip() for writer, start, end in writers if writer != "psize1"]
        site = {"source": anchor(CLIENT, text, call.start(), end), "writes": fields}
        if opcode in rows:
            if (rows[opcode]["name"], rows[opcode]["length"]) != (name, length):
                raise ValueError(f"inconsistent outbound sites for {opcode}")
            rows[opcode]["sites"].append(site)
        else:
            rows[opcode] = {"name": name, "id": opcode, "length": length,
                            "length_derivation": derivation, "sites": [site]}
    if len({row["name"] for row in rows.values()}) != len(rows):
        raise ValueError("outbound names are not unique")
    return [rows[opcode] for opcode in sorted(rows)]


def protocol_contract(java: Path) -> dict:
    for name, expected in SOURCE_HASHES.items():
        if sha256((java / name).read_bytes()) != expected:
            raise ValueError(f"source differs from pinned Java commit: {name}")
    text = (java / CLIENT).read_text()
    protocol = (java / PROTOCOL).read_text()
    if "this.login.p2(377);" not in text:
        raise ValueError("Java login does not declare revision 377")
    array = re.search(r"SERVERPROT_LENGTH\s*=\s*new int\[\]\s*\{([^}]+)\}", protocol)
    if array is None:
        raise ValueError("missing SERVERPROT_LENGTH")
    sizes = [int(value) for value in array[1].split(",")]
    # Java has one unreachable trailing zero. ptype is masked to an unsigned byte.
    if len(sizes) != 257 or sizes[256:] != [0] or any(size < -2 for size in sizes):
        raise ValueError("unexpected SERVERPROT_LENGTH shape")
    inbound = [{"id": opcode, "length": size,
                "source": anchor(PROTOCOL, protocol, array.start(), array.end()),
                "length_kind": {-1: "g1", -2: "g2"}.get(size, "fixed")}
               for opcode, size in enumerate(sizes[:256])]
    source = {"repository": "https://github.com/LostCityRS/Client-Java",
              "commit": SOURCE_COMMIT,
              "sha256": SOURCE_HASHES}
    return {"revision": 377, "source_pin": source,
            "inbound": inbound, "unreachable_java_sizes": sizes[256:],
            "outbound": outbound_rows(text)}


def jag_members(data: bytes) -> tuple[bool, list[dict]]:
    unpacked = int.from_bytes(data[:3], "big")
    packed = int.from_bytes(data[3:6], "big")
    if len(data) != packed + 6:
        raise ValueError("JAG outer length mismatch")
    whole = unpacked != packed
    payload = bz2.decompress(b"BZh1" + data[6:]) if whole else data[6:]
    if len(payload) != unpacked:
        raise ValueError("JAG unpacked length mismatch")
    count = int.from_bytes(payload[:2], "big")
    cursor = 2 + count * 10
    members = []
    for index in range(count):
        entry = payload[2 + index * 10:12 + index * 10]
        if len(entry) != 10:
            raise ValueError("truncated JAG member table")
        name_hash = int.from_bytes(entry[:4], "big", signed=True)
        raw_len = int.from_bytes(entry[4:7], "big")
        packed_len = int.from_bytes(entry[7:10], "big")
        encoded = payload[cursor:cursor + packed_len]
        raw = encoded if whole else bz2.decompress(b"BZh1" + encoded)
        if len(raw) != raw_len:
            raise ValueError("JAG member length mismatch")
        members.append({"index": index, "name_hash": name_hash, "length": raw_len,
                        "sha256": sha256(raw)})
        cursor += packed_len
    if cursor != len(payload):
        raise ValueError("JAG member data does not consume the archive")
    return whole, members


def cache_manifest(cache: Path) -> dict:
    archives = []
    for slot, name in enumerate(NAMES, 1):
        data = (cache / name).read_bytes()
        whole, members = jag_members(data)
        archives.append({"name": name, "crc_slot": slot, "length": len(data),
                         "crc32": zlib.crc32(data), "sha256": sha256(data),
                         "whole_archive_compressed": whole, "members": members})
    return {"revision": 377, "provenance": "Fairy Ring revision-377 engine client pack; not an untouched 2006 cache",
            "oracle": "Python stdlib bz2/hashlib/zlib, independent of Rust JagFile",
            "source_anchors": [f"{CLIENT}:1701-1735", f"{JAGFILE}:33-84", f"{ONDEMAND}:136-211"],
            "archives": archives}


def frame_fixtures() -> dict:
    # Literal vectors derived from Java readPacket:7216-7266, not Rust framing.
    return {"revision": 377, "source_anchors": [f"{CLIENT}:7216-7266", f"{PROTOCOL}:11"],
            "cases": [
                {"name": "fixed_logout", "opcode": 5, "frame_hex": "05", "payload_hex": "", "header_length": 1},
                {"name": "fixed_rebuild_normal", "opcode": 222, "frame_hex": "de01020403", "payload_hex": "01020403", "header_length": 1},
                {"name": "g1_message_game", "opcode": 63, "frame_hex": "3f0368690a", "payload_hex": "68690a", "header_length": 2},
                {"name": "g2_player_info", "opcode": 90, "frame_hex": "5a00020000", "payload_hex": "0000", "header_length": 3},
                {"name": "g2_region_pending", "opcode": 53, "frame_hex": "3500050102", "payload_hex": None, "header_length": 3},
            ]}


def products(java: Path, cache: Path) -> dict[Path, str]:
    contract = protocol_contract(java)
    rows = contract["outbound"]
    header = ["//! Generated revision-377 client-to-server packet metadata.",
              "//! Source: Client-Java Client.java, pinned in docs/revision-377/protocol-377.json.",
              "//! Regenerate with tools/generate_revision_377_contract.py. No revision dispatch is installed.",
              "", "use super::ClientProt;", "", "pub struct ClientProt377;", "", "impl ClientProt377 {"]
    for row in rows:
        header.append(f"    // {row['sites'][0]['source']}")
        header.extend([f"    pub const {row['name']}: ClientProt = ClientProt {{",
                       f"        id: {row['id']},", f"        length: {row['length']},", "    };"])
    header.extend(["", "    /// Every opcode emitted by the pinned Java client, ordered by wire ID.",
                   f"    pub const ALL: [ClientProt; {len(rows)}] = ["])
    header.extend(f"        Self::{row['name']}," for row in rows)
    header.extend(["    ];", "}", ""])
    sizes = ["// Generated from Client-Java Protocol.java:11 (SERVERPROT_LENGTH).",
             "// Java's unreachable entry 256 is recorded in protocol-377.json, not indexed by a byte.",
             "pub const SERVER_PROT_SIZES_377: [i32; 256] = ["]
    sizes.extend(f"    {row['length']}," for row in contract["inbound"])
    sizes.extend(["];", ""])
    return {
        ROOT / "crates/client/src/io/client_prot_377.rs": format_rust("\n".join(header)),
        ROOT / "crates/client/src/io/server_prot_sizes_377.inc.rs": format_rust("\n".join(sizes)),
        ROOT / "docs/revision-377/protocol-377.json": json.dumps(contract, indent=2) + "\n",
        ROOT / "crates/client/tests/fixtures/revision_377/cache.json": json.dumps(cache_manifest(cache), indent=2) + "\n",
        ROOT / "crates/client/tests/fixtures/revision_377/manifest.json": json.dumps(frame_fixtures(), indent=2) + "\n",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java-root", required=True, type=Path)
    parser.add_argument("--cache-dir", required=True, type=Path)
    parser.add_argument("--check", action="store_true", help="compare generated bytes without writing")
    args = parser.parse_args()
    outputs = products(args.java_root, args.cache_dir)
    for path, content in outputs.items():
        if args.check:
            if path.read_text() != content:
                raise SystemExit(f"FAIL: stale generated file: {path.relative_to(ROOT)}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
    contract_path = ROOT / "docs/revision-377/protocol-377.json"
    outbound_count = len(json.loads(outputs[contract_path])["outbound"])
    print(f"PASS: {'checked' if args.check else 'generated'} 256 inbound, "
          f"{outbound_count} outbound rows; 8 real JAG oracles")


if __name__ == "__main__":
    main()
