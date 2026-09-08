# Revision 289 source contract

Status: source-grounded offline contract with stage-3 production-enabled outbound
rows traced. This is not a live compatibility claim.

## Authority and provenance

Primary source is RuneWiki/openrs2-nonfree branch 289 at commit `0c00ef249546fada67b1f6eb8bbe01ea7c250c95`, entry `nonfree/client/src/main/java/client.java`. The source inventory and complete tracked-file hashes are recorded in the operator-supplied `deob-289-source-inventory.md` and `deob-289-source-provenance.json`.

Verified source hashes:

- `client.java`: `d03a34d8c965a426993f5a3e812bc566a80a18a91de65e994a257c49a5f5fea6`
- deob `nonfree/var/cache/deob/client.jar`: `3ec08f3e733f0f021d73f180300f00349cb01e4a50803ab5f9efe2af839f70f6`
- upstream `nonfree/lib/289.2005-01-17.jar`: `51593c692b5bb2087b1c508f3e6030c3a256d38192eecce515b6ea563d2bc058`

These are source/deob/JAR artifacts, not a verified host-loadable game-cache pairing. The intended cache manifest, compatible server endpoint, credentials/test authorization, and replay capture are absent. `vendor/client-java-289` at `6834c7255f559db5f1702b8b0e5e7286a7d61244` is corroboration only and is not promoted over the primary source.

## Framing and handshake

At `client.java:2553-2587`, the client reads one opcode byte, subtracts ISAAC when active, selects `Class17.anIntArray209[opcode]`, then handles fixed sizes, `-1` byte lengths, and `-2` big-endian `g2` lengths. It refuses to consume a frame until the declared payload is available. Player updates at `client.java:6150-6168` additionally require the final cursor to equal the declared packet size; this is a mandatory invariant for Rust.

Login at `client.java:8342-8398` sends request opcode 14 plus the five-bit name hash, receives eight server-seed bytes, then builds RSA plaintext in source order: block type `10`; four big-endian ISAAC seed words (two random words followed by the server-seed high/low words); `signlink.anInt928`; username Jagex string; password Jagex string. `method491(aBigInteger2, aBigInteger1)` applies the RSA operation. The subsequent login frame writes mode 16/18, RSA length, `255`, revision `289`, membership byte, nine cache/config index g4 values, and the RSA block. Response 2 enters the game; responses 3-14 and 16-21 are explicit error/retry outcomes. Response 15 clears packet/frame state without being equivalent to a successful logout. The RSA modulus/exponent values and operator-approved endpoint remain outside this public contract. The public fixture manifest contains a credential-free structural vector for this ordered plaintext; it is not a live login claim.

## Inbound contract

`protocol-289.json` enumerates all 256 decoded inbound IDs and their exact `Class17.anIntArray209` lengths. Named rows are source-derived only where the primary dispatch branch was traced: actor/player update (188), inventory full (107), inventory partial (76), region rebuild (219), varp/config (75, 97, 172), widgets/interface (59, 211, 252), logout (121), interface reset/set (127), and actor reset (201). Notably, opcode 172 is a zero-payload bulk varp sync, 55 opens two interfaces, and 127 reads a signed g2 interface id; these are not renamed to inventory, region, or logout. Other rows retain explicit unknown field descriptions rather than guessed schemas.

Important encodings:

- `g2`: unsigned big-endian two-byte value.
- `gsmart`: one byte for values below 128; otherwise unsigned `g2 - 0x8000`.
- variable frame lengths: `g1` for `-1`, `g2` for `-2`.
- actor updates: method212 local update bits, method185 g8 other-player count/movement, method172 11-bit new-player records with signed 5/5 offsets and 1-bit flags, method153 one/two-byte masks, and method128 ordered mask payloads (0x001/002/004/008/010/020/040/100/200/400); implementation must consume exactly the declared frame.
- inventory: revision-specific `g2` container/count shape and smart slot/item values; do not carry 274 widths into 289.
- widgets: component IDs and newline-terminated strings are cache/interface data, not public ABI constants.

## Outbound contract

Outbound IDs observed at `client.java` `method465(...)` call sites (including method160/method206/method216 wrappers) are listed in `protocol-289.json`.

Stage-3 production-enabled families have **source-ordered payload lengths and fields** promoted from primary write sequences (`method466`=p1, `method467`=p2, `method469`=p3, `method470`=p4, `method472`=p8) into both `protocol-289.json` and `ClientProt289`. Enabled families include:

- walk: MOVE_GAMECLICK 234, MOVE_MINIMAPCLICK 236, MOVE_OPCLICK 67 (variable)
- NPC/loc/object/player/held/inv-button ops (fixed lengths 2/4/6/8/12 as traced)
- IF_BUTTON 86, RESUME_PAUSEBUTTON 166, RESUME_P_COUNTDIALOG 180, CLOSE_MODAL 93
- MAP_BUILD_COMPLETE 214, NO_TIMEOUT 181, CHAT_SETMODE 161
- MESSAGE_PUBLIC 156 (p1 colour + p1 effect with Java prefixes wave/wave2/shake/scroll/slide → 1/2/3/4/5 + wordpack)
- SEND_SNAPSHOT 94 (report-abuse: p8 namehash + p1 reason + p1 mute via method472/method466)
- friend/ignore add/del: p8 userhash via method472
- EVENT_MOUSE_MOVE 229: p1 size + timed delta/absolute samples (method467/method469/method470) + psize1
- draw-path anticheat/tut: CYCLELOGIC1 130, CYCLELOGIC3 125, CYCLELOGIC6 255, TUT_CLICKSIDE 146
- remaining mapped table rows used by production emit (events, social, design, idle)

Zero-payload outbound rows use an explicit `(empty payload)` field marker (or empty list only when `length==0`); non-zero lengths must keep a non-empty ordered field list. Rows still without an independent field trace must not be newly enabled. Production emit routes through `Client::client_opcode` / `map_client_prot` (client interact sites and render/draw sites for the four previously bare opcodes). Unmapped 274 constants fail closed on R289. Length `"unknown"` is no longer acceptable for any production-enabled row.

## Existing Rust mapping

- `crates/client/src/io/packet.rs`: primitive g1/g2/gsmart/bit/RSA/ISAAC operations; keep as low-level primitives and add bounded error paths before production use.
- `crates/client/src/io/server_prot.rs` and `client_prot.rs`: current 274 tables; preserve public constants/default construction and add an explicit revision-selected profile.
- `crates/client/src/io/client_prot_289.rs`: additive 289 outbound table + `map_client_prot`.
- `crates/client/src/io/cache_289.rs`: offline synthetic config/interface fixtures through production `Cache::unpack` / `IfType::unpack`; JAG outer g3 sizes exclude the six-byte header.
- `crates/client/src/client/client.rs`, `game_shell.rs`: production lifecycle and packet dispatch seams; `client_opcode` selects revision outbound ids.
- `crates/client/src/render/draw.rs`: draw-path outbound emits also use `client_opcode`.
- `crates/client/src/login_rsa.rs`: existing RSA boundary; compare byte-for-byte against the 289 login sequence before changing it.
- `crates/client/src/io/client_stream.rs`: transport/read buffering; frame availability must be checked before dispatch.
- `crates/client/src/core/world.rs` and render modules: preserve existing scene ownership and `scene_state == 1` last-FBO freeze.

## Explicit unknowns and gates

1. No authoritative 289 game-cache manifest or server/cache pairing is available (authentic assets/render/scene proof gated). Offline synthetic config/interface records through production loaders are authorized and do not substitute for authentic pairing.
2. No approved live endpoint or credentials/test authorization is available.
3. Outbound production-enabled families now carry traced lengths/fields; further social/event edge cases beyond the mapped table remain fail-closed if unmapped.
4. Exact actor local-player index, region base/plane timing, widget IDs and full config/object definitions require primary-source tracing plus authentic cache evidence for live proof.
5. Live RSA/ISAAC compatibility still needs a capture or authorized endpoint, but the source-derived ordered plaintext structure is independently checked offline; compilation is not proof.

The accompanying fixtures are tiny public-safe byte cases with independently derived expected results. Each packet row separates opcode/frame bytes from payload bytes, records declared length versus actual cursor consumption, and uses structured pending/dispatch/error results. The actor case is a valid two-byte zero bitstream: local update bit 0, other count 0, no new-player records, no masks, then byte alignment. The login row is a credential-free structural oracle rather than packet bytes. Synthetic config/interface JAGs exercise production unpackers only; they are not live world definitions.
