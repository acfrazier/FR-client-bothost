#!/usr/bin/env python3
"""Append independent actor-mask fixture cases to the revision_289 manifest."""
from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "crates/client/tests/fixtures/revision_289/manifest.json"

CASES = [
    {
        "name": "npc_info_hitmark_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "410009019fff801005010a14",
        "payload_hex": "019fff801005010a14",
        "declared_payload_length": 9,
        "expected_consumed_length": 12,
        "expected_cursor": 9,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_hitmark",
            "health": 10,
            "total_health": 20,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 HITMARK 0x10)",
        ],
        "oracle_derivation": "Existing npc: g8 count 1, info 1, op 0, 14-bit 16383 sentinel, mask 0x10 HITMARK, four g1 damage/type/health/total. Independently packed; combat timer stays loop_cycle+400.",
    },
    {
        "name": "npc_info_anim_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "410008019fff8002123403",
        "payload_hex": "019fff8002123403",
        "declared_payload_length": 8,
        "expected_consumed_length": 11,
        "expected_cursor": 8,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_anim",
            "primary_anim": 4660,
            "primary_anim_delay": 3,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 ANIM 0x2)",
        ],
        "oracle_derivation": "Mask 0x02 ANIM g2=0x1234 + g1 delay 3 on existing npc extended slot.",
    },
    {
        "name": "npc_info_say_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "410008019fff8008686900",
        "payload_hex": "019fff8008686900",
        "declared_payload_length": 8,
        "expected_consumed_length": 11,
        "expected_cursor": 8,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_say",
            "chat_message": "hi",
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 SAY 0x8)",
        ],
        "oracle_derivation": "Mask 0x08 SAY gjstr \"hi\" null-terminated.",
    },
    {
        "name": "npc_info_facesquare_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "410009019fff8080000a000b",
        "payload_hex": "019fff8080000a000b",
        "declared_payload_length": 9,
        "expected_consumed_length": 12,
        "expected_cursor": 9,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_facesquare",
            "face_square_x": 10,
            "face_square_z": 11,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 FACESQUARE 0x80)",
        ],
        "oracle_derivation": "Mask 0x80 FACESQUARE two g2 values 10 and 11.",
    },
    {
        "name": "npc_info_spotanim_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "41000b019fff8040000700010002",
        "payload_hex": "019fff8040000700010002",
        "declared_payload_length": 11,
        "expected_consumed_length": 14,
        "expected_cursor": 11,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_spotanim",
            "spotanim_id": 7,
            "spotanim_height": 1,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 SPOTANIM 0x40)",
        ],
        "oracle_derivation": "Mask 0x40 SPOTANIM g2 id 7 + g4 packed height/delay 0x00010002.",
    },
    {
        "name": "npc_info_hitmark2_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "410009019fff800109021e28",
        "payload_hex": "019fff800109021e28",
        "declared_payload_length": 9,
        "expected_consumed_length": 12,
        "expected_cursor": 9,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_hitmark2",
            "health": 30,
            "total_health": 40,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 HITMARK2 0x1)",
        ],
        "oracle_derivation": "Mask 0x01 HITMARK2 four g1; timer remains loop_cycle+400 (not Java +300).",
    },
    {
        "name": "npc_info_changetype_mask",
        "opcode": 65,
        "length_kind": "g2",
        "frame_hex": "410007019fff80200005",
        "payload_hex": "019fff80200005",
        "declared_payload_length": 7,
        "expected_consumed_length": 10,
        "expected_cursor": 7,
        "expected_result": {
            "status": "dispatch",
            "state": "npc_changetype",
            "type_id_read": 5,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[65] = -2)",
            "client.java:11868-11970 (method222 CHANGETYPE 0x20)",
        ],
        "oracle_derivation": "Mask 0x20 CHANGETYPE g2 type id 5; empty cache leaves type unset after consume.",
    },
    {
        "name": "actor_update_say_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc0007801ffc08796f00",
        "payload_hex": "801ffc08796f00",
        "declared_payload_length": 7,
        "expected_consumed_length": 10,
        "expected_cursor": 7,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_say",
            "chat_message": "yo",
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 SAY 0x8)",
        ],
        "oracle_derivation": "Local mask-only bitstream + mask 0x08 SAY gjstr \"yo\". Independent of appearance|face_entity fixture.",
    },
    {
        "name": "actor_update_hitmark_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc0008801ffc1004003264",
        "payload_hex": "801ffc1004003264",
        "declared_payload_length": 8,
        "expected_consumed_length": 11,
        "expected_cursor": 8,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_hitmark",
            "health": 50,
            "total_health": 100,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 HITMARK 0x10)",
        ],
        "oracle_derivation": "Mask 0x10 HITMARK four g1; combat_cycle uses shared +400 timer.",
    },
    {
        "name": "actor_update_anim_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc0007801ffc02001102",
        "payload_hex": "801ffc02001102",
        "declared_payload_length": 7,
        "expected_consumed_length": 10,
        "expected_cursor": 7,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_anim",
            "primary_anim": 17,
            "primary_anim_delay": 2,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 ANIM 0x2)",
        ],
        "oracle_derivation": "Mask 0x02 ANIM g2=0x0011 + g1 delay 2.",
    },
    {
        "name": "actor_update_facesquare_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc0008801ffc2001020304",
        "payload_hex": "801ffc2001020304",
        "declared_payload_length": 8,
        "expected_consumed_length": 11,
        "expected_cursor": 8,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_facesquare",
            "face_square_x": 258,
            "face_square_z": 772,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 FACESQUARE 0x20)",
        ],
        "oracle_derivation": "Mask 0x20 FACESQUARE two g2.",
    },
    {
        "name": "actor_update_spotanim_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc000b801ffc8001000900000005",
        "payload_hex": "801ffc8001000900000005",
        "declared_payload_length": 11,
        "expected_consumed_length": 14,
        "expected_cursor": 11,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_spotanim",
            "spotanim_id": 9,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 SPOTANIM 0x100)",
        ],
        "oracle_derivation": "Big-update mask bytes 0x80|0x01 = SPOTANIM; g2 id 9 + g4 delay/height.",
    },
    {
        "name": "actor_update_exactmove_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc000e801ffc800201020304000a001405",
        "payload_hex": "801ffc800201020304000a001405",
        "declared_payload_length": 14,
        "expected_consumed_length": 17,
        "expected_cursor": 14,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_exactmove",
            "exact_start_x": 1,
            "exact_start_z": 2,
            "exact_end_x": 3,
            "exact_end_z": 4,
            "exact_move_facing": 5,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 EXACTMOVE 0x200)",
        ],
        "oracle_derivation": "Big-update 0x80|0x02 EXACTMOVE: 4 g1 coords + g2 end + g2 start + g1 facing. Not hit/health.",
    },
    {
        "name": "actor_update_hitmark2_mask",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc0009801ffc800406014650",
        "payload_hex": "801ffc800406014650",
        "declared_payload_length": 9,
        "expected_consumed_length": 12,
        "expected_cursor": 9,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_hitmark2",
            "health": 70,
            "total_health": 80,
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 HITMARK2 0x400)",
        ],
        "oracle_derivation": "Big-update 0x80|0x04 HITMARK2 secondary hit four g1.",
    },
    {
        "name": "actor_update_chat_mask_empty",
        "opcode": 188,
        "length_kind": "g2",
        "frame_hex": "bc0008801ffc4001000000",
        "payload_hex": "801ffc4001000000",
        "declared_payload_length": 8,
        "expected_consumed_length": 11,
        "expected_cursor": 8,
        "expected_result": {
            "status": "dispatch",
            "state": "actor_chat_empty",
            "error": None,
        },
        "source_anchors": [
            "Class17.java:11 (length[188] = -2)",
            "client.java:8021-8054 (method128 CHAT 0x40)",
        ],
        "oracle_derivation": "Mask 0x40 CHAT g2 colour_effect + g1 type + g1 length 0 (empty wordpack body) proves cursor consume independent of appearance fixture.",
    },
]


def main() -> None:
    data = json.loads(MANIFEST.read_text())
    names = {c["name"] for c in data["cases"]}
    added = 0
    for case in CASES:
        if case["name"] in names:
            # replace existing
            data["cases"] = [c for c in data["cases"] if c["name"] != case["name"]]
        data["cases"].append(case)
        added += 1
        names.add(case["name"])
    MANIFEST.write_text(json.dumps(data, indent=2) + "\n")
    print(f"wrote {added} cases; total {len(data['cases'])}")


if __name__ == "__main__":
    main()
