#!/usr/bin/env python3
"""Pack independent NPC/player mask fixture bytes matching production gbit layout."""
from __future__ import annotations


class BitOut:
    def __init__(self) -> None:
        self.bits: list[int] = []

    def pbit(self, n: int, v: int) -> None:
        for i in range(n - 1, -1, -1):
            self.bits.append((v >> i) & 1)

    def to_bytes(self) -> bytes:
        out: list[int] = []
        b = 0
        k = 0
        for bit in self.bits:
            b = (b << 1) | bit
            k += 1
            if k == 8:
                out.append(b)
                b = 0
                k = 0
        if k:
            b <<= 8 - k
            out.append(b)
        return bytes(out)


def npc_hdr() -> bytes:
    bo = BitOut()
    bo.pbit(8, 1)  # count
    bo.pbit(1, 1)  # info
    bo.pbit(2, 0)  # op extended
    bo.pbit(14, 16383)  # new-vis sentinel
    return bo.to_bytes()


def player_hdr() -> bytes:
    bo = BitOut()
    bo.pbit(1, 1)  # local info
    bo.pbit(2, 0)  # op mask-only
    bo.pbit(8, 0)  # old-vis count
    bo.pbit(11, 2047)  # new-vis sentinel
    return bo.to_bytes()


def frame(op: int, payload: bytes) -> bytes:
    return bytes([op, (len(payload) >> 8) & 0xFF, len(payload) & 0xFF]) + payload


def main() -> None:
    assert npc_hdr().hex() == "019fff80"
    assert player_hdr().hex() == "801ffc"
    cases = [
        ("npc_info_hitmark_mask", 65, npc_hdr() + bytes([0x10, 5, 1, 10, 20])),
        ("npc_info_anim_mask", 65, npc_hdr() + bytes([0x02, 0x12, 0x34, 3])),
        ("npc_info_say_mask", 65, npc_hdr() + bytes([0x08]) + b"hi\x00"),
        ("npc_info_facesquare_mask", 65, npc_hdr() + bytes([0x80, 0x00, 0x0A, 0x00, 0x0B])),
        ("npc_info_spotanim_mask", 65, npc_hdr() + bytes([0x40, 0x00, 0x07, 0x00, 0x01, 0x00, 0x02])),
        ("npc_info_hitmark2_mask", 65, npc_hdr() + bytes([0x01, 9, 2, 30, 40])),
        ("npc_info_changetype_mask", 65, npc_hdr() + bytes([0x20, 0x00, 0x05])),
        ("actor_update_say_mask", 188, player_hdr() + bytes([0x08]) + b"yo\x00"),
        ("actor_update_hitmark_mask", 188, player_hdr() + bytes([0x10, 4, 0, 50, 100])),
        ("actor_update_anim_mask", 188, player_hdr() + bytes([0x02, 0x00, 0x11, 2])),
        ("actor_update_facesquare_mask", 188, player_hdr() + bytes([0x20, 0x01, 0x02, 0x03, 0x04])),
        (
            "actor_update_spotanim_mask",
            188,
            player_hdr() + bytes([0x80, 0x01, 0x00, 0x09, 0x00, 0x00, 0x00, 0x05]),
        ),
        (
            "actor_update_exactmove_mask",
            188,
            player_hdr() + bytes([0x80, 0x02, 1, 2, 3, 4, 0x00, 0x0A, 0x00, 0x14, 5]),
        ),
        (
            "actor_update_hitmark2_mask",
            188,
            player_hdr() + bytes([0x80, 0x04, 6, 1, 70, 80]),
        ),
        (
            "actor_update_chat_mask_empty",
            188,
            player_hdr() + bytes([0x40, 0x01, 0x00, 0, 0]),
        ),
    ]
    for name, op, payload in cases:
        fr = frame(op, payload)
        print(f"{name}\t{payload.hex()}\t{fr.hex()}\t{len(payload)}")


if __name__ == "__main__":
    main()
