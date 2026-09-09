#!/usr/bin/env python3
"""Promote ClientProt289 traced outbound lengths/fields into protocol-289.json."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PROTO = ROOT / "docs/revision-289/protocol-289.json"

# id -> (name, length, fields, anchors, confidence)
# Source: primary client.java method465/method160/method206 write sequences
# (openrs2-nonfree 0c00ef24) as encoded in crates/client/src/io/client_prot_289.rs
ROWS = {
    4: ("OPOBJ2", 6, ["p2 sceneX", "p2 sceneZ", "p2 objId"], ["client.java method216 method465(4) + three method467"], "verified-id-length-fields"),
    10: ("OPLOC1", 6, ["p2 sceneX", "p2 sceneZ", "p2 locId"], ["client.java method160 method465(10) + three method467"], "verified-id-length-fields"),
    13: ("OPPLAYER3", 2, ["p2 playerIndex"], ["client.java method216 method465(13) + method467"], "verified-id-length-fields"),
    16: ("OPPLAYERU", 8, ["p2 playerIndex", "p2 useObj", "p2 useSlot", "p2 useCom"], ["client.java method216 method465(16) + four method467"], "verified-id-length-fields"),
    21: ("OPNPC2", 2, ["p2 npcIndex"], ["client.java method216 method465(21) + method467"], "verified-id-length-fields"),
    22: ("OPOBJ5", 6, ["p2 sceneX", "p2 sceneZ", "p2 objId"], ["client.java method216 method465(22) + three method467"], "verified-id-length-fields"),
    27: ("IDK_SAVEDESIGN", 13, ["p1 gender", "p1 kit[7]", "p1 colour[5]"], ["client.java design accept method465(27)"], "verified-id-length-fields"),
    30: ("OPNPC4", 2, ["p2 npcIndex"], ["client.java method216 method465(30) + method467"], "verified-id-length-fields"),
    34: ("CLIENT_CHEAT", -1, ["p1 size", "pjstr command"], ["client.java chat :: branch method465(34)"], "verified-id-length-fields"),
    40: ("OPHELD3", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(40) + three method467"], "verified-id-length-fields"),
    44: ("INV_BUTTON1", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(44) + three method467"], "verified-id-length-fields"),
    45: ("OPLOC2", 6, ["p2 sceneX", "p2 sceneZ", "p2 locId"], ["client.java method160 method465(45) + three method467"], "verified-id-length-fields"),
    46: ("ANTICHEAT_OPLOGIC5", 1, ["p1 pad"], ["client.java method465(46)"], "verified-id-length-fields"),
    49: ("ANTICHEAT_OPLOGIC4", 1, ["p1 pad"], ["client.java method465(49)"], "verified-id-length-fields"),
    51: ("OPPLAYER2", 2, ["p2 playerIndex"], ["client.java method216 method465(51) + method467"], "verified-id-length-fields"),
    53: ("OPLOC4", 6, ["p2 sceneX", "p2 sceneZ", "p2 locId"], ["client.java method160 method465(53) + three method467"], "verified-id-length-fields"),
    55: ("OPOBJU", 12, ["p2 sceneX", "p2 sceneZ", "p2 objId", "p2 useObj", "p2 useSlot", "p2 useCom"], ["client.java method216 method465(55)"], "verified-id-length-fields"),
    67: ("MOVE_OPCLICK", -1, ["p1 size", "p1 ctrl", "p2 absX", "p2 absZ", "step deltas"], ["client.java method206 method465(67)"], "verified-id-length-fields"),
    69: ("OPPLAYER5", 2, ["p2 playerIndex"], ["client.java method216 method465(69) + method467"], "verified-id-length-fields"),
    73: ("ANTICHEAT_OPLOGIC6", 2, ["p2 pad"], ["client.java method465(73)"], "verified-id-length-fields"),
    76: ("OPHELD1", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(76) + three method467"], "verified-id-length-fields"),
    79: ("OPHELD5", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(79) + three method467"], "verified-id-length-fields"),
    81: ("ANTICHEAT_OPLOGIC2", 2, ["p2 pad"], ["client.java method465(81)"], "verified-id-length-fields"),
    85: ("ANTICHEAT_CYCLELOGIC5", 0, ["(empty payload)"], ["client.java method465(85)"], "verified-id-length-fields"),
    86: ("IF_BUTTON", 2, ["p2 componentId"], ["client.java method465(86) + method467"], "verified-id-length-fields"),
    88: ("ANTICHEAT_OPLOGIC9", 3, ["p3 pad"], ["client.java method465(88)"], "verified-id-length-fields"),
    93: ("CLOSE_MODAL", 0, ["(empty payload)"], ["client.java closeModal method465(93)"], "verified-id-length-fields"),
    94: ("SEND_SNAPSHOT", 10, ["p8 namehash", "p1 reason", "p1 mute"], ["client.java report abuse method465(94)+method472+two method466"], "verified-id-length-fields"),
    97: ("OPOBJ1", 6, ["p2 sceneX", "p2 sceneZ", "p2 objId"], ["client.java method216 method465(97) + three method467"], "verified-id-length-fields"),
    107: ("MESSAGE_PRIVATE", -1, ["p1 size", "p8 target", "wordpacked text"], ["client.java social PM method465(107)"], "verified-id-length-fields"),
    108: ("OPNPCT", 4, ["p2 npcIndex", "p2 spell"], ["client.java method216 method465(108)"], "verified-id-length-fields"),
    110: ("OPOBJ3", 6, ["p2 sceneX", "p2 sceneZ", "p2 objId"], ["client.java method216 method465(110) + three method467"], "verified-id-length-fields"),
    111: ("INV_BUTTON2", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(111) + three method467"], "verified-id-length-fields"),
    112: ("OPHELDT", 8, ["p2 obj", "p2 slot", "p2 com", "p2 spell"], ["client.java method216 method465(112)"], "verified-id-length-fields"),
    122: ("ANTICHEAT_OPLOGIC3", 4, ["p4 pad"], ["client.java method465(122)"], "verified-id-length-fields"),
    124: ("INV_BUTTON3", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(124) + three method467"], "verified-id-length-fields"),
    125: ("ANTICHEAT_CYCLELOGIC3", 1, ["p1 pad"], ["client.java minimap cycle method465(125)"], "verified-id-length-fields"),
    126: ("OPLOC5", 6, ["p2 sceneX", "p2 sceneZ", "p2 locId"], ["client.java method160 method465(126) + three method467"], "verified-id-length-fields"),
    130: ("ANTICHEAT_CYCLELOGIC1", -1, ["p1 size", "variable random blob"], ["client.java addProjectiles method465(130)"], "verified-id-length-fields"),
    133: ("ANTICHEAT_OPLOGIC7", 4, ["p4 pad"], ["client.java method465(133)"], "verified-id-length-fields"),
    137: ("ANTICHEAT_CYCLELOGIC4", 1, ["p1 pad"], ["client.java method465(137)"], "verified-id-length-fields"),
    138: ("OPPLAYERT", 4, ["p2 playerIndex", "p2 spell"], ["client.java method216 method465(138)"], "verified-id-length-fields"),
    145: ("IDLE_TIMER", 0, ["(empty payload)"], ["client.java method465(145)"], "verified-id-length-fields"),
    146: ("TUT_CLICKSIDE", 1, ["p1 tab"], ["client.java drawIcons method465(146)"], "verified-id-length-fields"),
    147: ("OPOBJ4", 6, ["p2 sceneX", "p2 sceneZ", "p2 objId"], ["client.java method216 method465(147) + three method467"], "verified-id-length-fields"),
    149: ("EVENT_APPLET_FOCUS", 1, ["p1 focused"], ["client.java method465(149)"], "verified-id-length-fields"),
    154: ("ANTICHEAT_CYCLELOGIC2", -1, ["p1 size", "variable random blob"], ["client.java method465(154)"], "verified-id-length-fields"),
    156: ("MESSAGE_PUBLIC", -1, ["p1 size", "p1 colour", "p1 effect", "wordpacked text"], ["client.java chat method465(156)"], "verified-id-length-fields"),
    160: ("OPNPCU", 8, ["p2 npcIndex", "p2 useObj", "p2 useSlot", "p2 useCom"], ["client.java method216 method465(160)"], "verified-id-length-fields"),
    161: ("CHAT_SETMODE", 3, ["p1 public", "p1 private", "p1 trade"], ["client.java chat mode method465(161)"], "verified-id-length-fields"),
    166: ("RESUME_PAUSEBUTTON", 2, ["p2 componentId"], ["client.java method465(166) + method467"], "verified-id-length-fields"),
    168: ("ANTICHEAT_OPLOGIC8", 1, ["p1 pad"], ["client.java method465(168)"], "verified-id-length-fields"),
    177: ("OPHELD2", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(177) + three method467"], "verified-id-length-fields"),
    178: ("OPNPC3", 2, ["p2 npcIndex"], ["client.java method216 method465(178) + method467"], "verified-id-length-fields"),
    180: ("RESUME_P_COUNTDIALOG", 4, ["p4 amount"], ["client.java handleInputKey count dialog method465(180) + method470"], "verified-id-length-fields"),
    181: ("NO_TIMEOUT", 0, ["(empty payload)"], ["client.java method465(181)"], "verified-id-length-fields"),
    184: ("OPLOCU", 12, ["p2 sceneX", "p2 sceneZ", "p2 locId", "p2 useObj", "p2 useSlot", "p2 useCom"], ["client.java method160 method465(184) + extras"], "verified-id-length-fields"),
    189: ("OPPLAYER4", 2, ["p2 playerIndex"], ["client.java method216 method465(189) + method467"], "verified-id-length-fields"),
    191: ("OPHELD4", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(191) + three method467"], "verified-id-length-fields"),
    192: ("IGNORELIST_ADD", 8, ["p8 userhash"], ["client.java method465(192) + method472"], "verified-id-length-fields"),
    193: ("EVENT_CAMERA_POSITION", 4, ["p2 pitch", "p2 yaw"], ["client.java:5915-5920"], "verified-id-length-fields"),
    195: ("ANTICHEAT_OPLOGIC1", 4, ["p4 pad"], ["client.java method465(195)"], "verified-id-length-fields"),
    196: ("OPLOC3", 6, ["p2 sceneX", "p2 sceneZ", "p2 locId"], ["client.java method160 method465(196) + three method467"], "verified-id-length-fields"),
    200: ("OPHELDU", 12, ["p2 obj", "p2 slot", "p2 com", "p2 useObj", "p2 useSlot", "p2 useCom"], ["client.java method216 method465(200)"], "verified-id-length-fields"),
    203: ("FRIENDLIST_DEL", 8, ["p8 userhash"], ["client.java method465(203) + method472"], "verified-id-length-fields"),
    214: ("MAP_BUILD_COMPLETE", 0, ["(empty payload)"], ["client.java map_build method465(214)"], "verified-id-length-fields"),
    218: ("OPLOCT", 8, ["p2 sceneX", "p2 sceneZ", "p2 locId", "p2 spell"], ["client.java method160 method465(218) + spell"], "verified-id-length-fields"),
    220: ("OPPLAYER1", 2, ["p2 playerIndex"], ["client.java method216 method465(220) + method467"], "verified-id-length-fields"),
    224: ("EVENT_MOUSE_CLICK", 4, ["p4 packed elapsed/button/linear-coordinate"], ["client.java:5882-5907"], "verified-id-length-fields"),
    227: ("INV_BUTTON5", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(227) + three method467"], "verified-id-length-fields"),
    229: ("EVENT_MOUSE_MOVE", -1, ["p1 size", "sample p2 (dt<<12)|((dx+32)<<6)|(dy+32) when dt<8 and -32<=dx,dy<=31", "sample p3 (dt<<19)+coord+0x800000 when dt<8 and either delta outside range", "sample p4 (dt<<19)+coord-0x40000000 when dt>=8", "coord=clampedY*765+clampedX; raw(-1,-1) sentinel=524287; duplicate count saturates2047", "psize1; authorized forward payload<240 whole-sample budget, leftovers retained"], ["client.java:5820-5878; Class11.java:19-23,45-52; cleanup-h-report.md authorized framing correction"], "verified-id-length-fields"),
    232: ("ANTICHEAT_CYCLELOGIC7", 0, [], ["client.java:6023-6027"], "verified-id-length-fields"),
    234: ("MOVE_GAMECLICK", -1, ["p1 size", "p1 ctrl", "p2 absX", "p2 absZ", "step deltas"], ["client.java method206 method465(234)"], "verified-id-length-fields"),
    235: ("FRIENDLIST_ADD", 8, ["p8 userhash"], ["client.java method465(235) + method472"], "verified-id-length-fields"),
    236: ("MOVE_MINIMAPCLICK", -1, ["p1 size+14", "p1 ctrl", "p2 absX", "p2 absZ", "steps", "14-byte minimap tail"], ["client.java method206 method465(236)"], "verified-id-length-fields"),
    241: ("OPOBJT", 8, ["p2 sceneX", "p2 sceneZ", "p2 objId", "p2 spell"], ["client.java method216 method465(241)"], "verified-id-length-fields"),
    247: ("OPNPC5", 2, ["p2 npcIndex"], ["client.java method216 method465(247) + method467"], "verified-id-length-fields"),
    248: ("INV_BUTTON4", 6, ["p2 obj", "p2 slot", "p2 com"], ["client.java method216 method465(248) + three method467"], "verified-id-length-fields"),
    251: ("IGNORELIST_DEL", 8, ["p8 userhash"], ["client.java method465(251) + method472"], "verified-id-length-fields"),
    252: ("OPNPC1", 2, ["p2 npcIndex"], ["client.java method216 method465(252) + method467"], "verified-id-length-fields"),
    253: ("INV_BUTTOND", 7, ["p2 com", "p2 fromSlot", "p2 toSlot", "p1 mode"], ["client.java inv drag method465(253)"], "verified-id-length-fields"),
    255: ("ANTICHEAT_CYCLELOGIC6", 1, ["p1 pad"], ["client.java addPlayers arrival method465(255)"], "verified-id-length-fields"),
}


def main():
    data = json.loads(PROTO.read_text())
    by_id = {row["id"]: row for row in data["outbound"]}
    updated = 0
    added = 0
    for oid, (name, length, fields, anchors, conf) in sorted(ROWS.items()):
        row = {
            "name": name,
            "id": oid,
            "length": length,
            "fields": fields,
            "source_anchors": anchors,
            "confidence": conf,
            "production_enabled": True,
            "notes": "Stage-3 source-traced payload; ClientProt289 + production client_opcode/map_client_prot.",
        }
        if oid in by_id:
            # preserve any extra keys that are not being replaced
            old = by_id[oid]
            for k in old:
                if k not in row and k not in ("length", "fields", "confidence", "source_anchors", "name", "notes"):
                    row[k] = old[k]
            by_id[oid] = row
            updated += 1
        else:
            by_id[oid] = row
            added += 1
    data["outbound"] = [by_id[i] for i in sorted(by_id)]
    # still-unknown check
    unknown = [r for r in data["outbound"] if r.get("length") == "unknown"]
    PROTO.write_text(json.dumps(data, indent=2) + "\n")
    print(f"updated={updated} added={added} total_outbound={len(data['outbound'])} still_unknown={len(unknown)}")
    if unknown:
        print("unknown ids:", [r["id"] for r in unknown])


if __name__ == "__main__":
    main()
