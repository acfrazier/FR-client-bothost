# Revision 289 whole-branch re-review (t_615aa9ac)

## Verdict: OFFLINE ACCEPTED (bounded)

Independent Grok 4.6 whole-branch re-review of `codex/revision-289-client`
after correction t_b04f979b (same-card reviewer approved 0e3b6a7). This is
**not** whole-client or live acceptance. The prior whole-branch rejection of
HEAD `0030afb` (t_77d35bfb, `docs/revision-289/branch-review.md`) is
**preserved**; it is not rewritten.

Both t_77d35bfb blocking defects are closed on reviewed HEAD `0e3b6a7`.
No new available-source offline blockers were found. Green compilation is
not protocol proof; the independent native suite and source-packed goldens
are.

| Field | Value |
| --- | --- |
| Reviewer task | t_615aa9ac |
| Model / provider | grok-4.6 / xai-oauth (profile branchreviewer defaults; no overrides) |
| Reviewed HEAD | `0e3b6a719c25b134473b94023807d3c3f90e8d68` |
| Prior rejected HEAD | `0030afbae662ee2b9939a519a23590f5c77fce3f` (receipt preserved) |
| Correction | t_b04f979b, same-card reviewer approved (Grok 4.5, artifact lens) |
| Branch | `codex/revision-289-client` |
| Base (prepared) | `716f79c762b58daba030ceb64ba208c7f2a0ffb0` |
| Rendering base | `4f2048ea10f75b3bb92ff45610b35ba7313b0308` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| CARGO_TARGET_DIR | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by reviewer | none |
| Follow-up correction cards | none |

Do not treat predecessor same-card approvals as live acceptance.

## Predecessor receipts (preserved)

- Source contract d2c6318: grok-4.5 round 4 approved, bounded evidence.
- Stage 1 ad68b99 accepted.
- Stage 2 e8ec353: grok-4.5 round 2 approved; opcode 65 NPC fields left unknown at that time.
- Corrective t_058241ac f766c9a accepted (strict inv end + session revision).
- Stage 3 t_448637e1: d441614 / run496 **changes_requested**; a00a641 execution-lens approved; run495 implementer self-review is **not** Grok4.5 acceptance.
- t_5f3a92c0 grok-4.5 **REJECTED** a00a641 (`docs/revision-289/stage3-corrective-review.md`).
- t_c2922712 grok-4.5 artifact-lens **approved** 0030afb for the four stage3-corrective defects only (R289 oracles).
- t_77d35bfb grok-4.6 **REJECTED** 0030afb (`docs/revision-289/branch-review.md`). **Preserve.**
- t_b04f979b grok-4.5 artifact-lens **approved** 0e3b6a7 for the two t_77d35bfb blockers only. Not branch acceptance.

## Independent tests (this run)

No host tests. No live hosts.

| Command | Result |
| --- | --- |
| `python3 tools/verify_revision_289_contract.py` | PASS 256 inbound, 82 outbound, 50 fixtures |
| `cargo test -p client --test revision_289_stage3` | 23 passed |
| `cargo test -p client --lib` | 70 passed |
| `cargo test -p client --test revision_289_stage1` | 26 passed |
| `cargo test -p client --test revision_289_stage2` | 30 passed |
| `cargo test -p client --test do_action` | 13 passed |
| `cargo test -p client --test walk` | 6 passed |
| `cargo test -p client --test prot` | 2 passed |
| `cargo test -p client --test logout` | 7 passed |
| `cargo test -p client --test from_shared` | 6 passed |
| `cargo test -p client --test player_info` | 1 passed |
| `cargo test -p client --test login_rsa` | 2 passed |
| `cargo test -p client --test gens` | 12 passed |
| `cargo test -p client --test server_packets` | 19 passed |
| `cargo test -p client --lib freeze_last_scene` | 1 passed |
| `cargo check -p client -p client-play` | ok |

Native suite is green. Green tests do not prove live 289 login/scene/action/logout.

## Closed t_77d35bfb blockers

### 1. Preserve-274 MESSAGE_PUBLIC effects — CLOSED

`Client::handle_chat_input` (`client.rs` 8684–8715) is revision-gated.

- Default `Client::new` / `client_274()` is `ClientRevision::R274`. Sequential
  ifs: `wave:` → 1, `scroll:` → 2. No wave2/shake/slide on 274.
- R289 else-if table: wave2/wave/shake/scroll/slide → 2/1/3/4/5.
- Production test `default_revision_message_public_scroll_wave_effects`
  asserts `Client::new` scroll→2 and wave→1 (not inferred from the R289 golden).
- `r289_message_public_effect_prefixes_match_java` retained (five effects).

Cold Java `client.java:10907-10921`: else-if wave=1, wave2=2, shake=3,
scroll=4, slide=5, opcode 156. Rust R289 checks wave2 before wave; Java
checks wave then wave2. With the colon suffixes the two orders are
equivalent (`"wave2:".startsWith("wave:")` is false). Not a defect.

Opcode remap still goes through `client_opcode`; 274 MESSAGE_PUBLIC id is
the public table. Effect byte is the preserve-274 surface and now matches
client-ts sequential prefixes on default construction.

### 2. Opcode 65 NPC field contract + player 0x200 gloss — CLOSED

`protocol-289.json` inbound 65 is `npc_info`, confidence
`verified-length-and-fields`, fields method187/226/124/222 with Java
anchors. Production `dispatch_packet_289` still calls shared `get_npc_pos`
(`client.rs` 6697–6700).

Cold comparison of primary Java (hash
`d03a34d8c965a426993f5a3e812bc566a80a18a91de65e994a257c49a5f5fea6`) against
Rust this run:

- Old-vis method226: 8-bit count, 1-bit info, 2-bit op, 3-bit walk / 3-bit
  run — match.
- New-vis method124: `bit_pos+21 < psize*8`, 14-bit index, sentinel 16383,
  11-bit type, signed 5/5, 1 jump, 1 extended — match.
- Mask bits method222: 0x1 HITMARK2, 0x2 ANIM, 0x4 FACEENTITY, 0x8 SAY,
  0x10 HITMARK, 0x20 CHANGETYPE, 0x40 SPOTANIM, 0x80 FACESQUARE — match.
- End cursor `pos == psize` then null-list check — match.

Inbound 188 method128 `0x200` gloss is exact-move (4×g1 + g2 + g2 + g1);
`0x400` is secondary hit. Java `client.java:5237-5254` confirms that
layout. Rust `EXACTMOVE=0x200` / `HITMARK2=0x400` and the remaining-mask
goldens assert start/end/facing and secondary health, not hit/health on
0x200.

Independent source-packed headers (`tools/pack_actor_mask_fixtures.py`
BitOut, not a Rust encoder dump): NPC `019fff80`, player `801ffc`. This
review recomputed those bitstreams against method226/212/185/172 widths.
Production tests `npc_info_remaining_masks_independent` and
`actor_update_remaining_masks_independent` assert published fields
(health/anim/say/facesquare/spotanim/exact-move/hitmark2) and
`pos == psize` through `handle_packet`. Not table self-agreement.

Shared HITMARK client timer stays `loop_cycle+400` (Java `anInt396+300`).
Required preserve; not rewritten.

## Cold-question results (non-blocking unless noted)

### Packet-size / reused buffers / malformed complete frames

`read_packet` copies exactly `psize` bytes at `pos=0` into the reused `in`
buffer. Actor decoders require `buf.pos == size` then logout. Inventory
stage1 has psize-bound / zero-publication tests
(`overlong_289_inv_full_exact_end_required_zero_publication`,
`truncated_inventory_full_does_not_partially_publish`,
`production_buffer_truncated_inv_*`). Player truncated update:
`actor_update_truncated_player_info_no_partial_world`. Unknown opcode logs
out (`server_packets` / stage2 `untraced_opcode_still_fail_closed`). Widget
`gjstr` still reads the allocated packet buffer (Java-like), not a separate
psize cap.

### ISAAC / login

RSA plaintext order (10, four seed words, uid, user, pass) is offline-proven
(`login_rsa_plaintext_structure_ordered`). Outer wrapper 16/18|255|p2(289)|…
is proven on a loopback socket. Java inbound Isaac seed `+= 50` matches Rust
`wrapping_add(50)`. That is a seed transform only; it does **not** prove
encrypted-login correspondence against a live modulus/capture. Live RSA/ISAAC
remains **live-unproven**.

### Scene / cache / freeze

Region rebuild sets `scene_state = 1` and `awaiting_player_info`
(`region_scene_base_rebuild`). `check_scene` assigns `scene_state = 2` only
after location/map readiness and not while awaiting player info. Synthetic
cache does not count as scene-build proof. `freeze_last_scene`
(`kind==Game && scene_state==1`) is unchanged vs 4f2048e (`gpu.rs` 1581–1582;
`cargo test freeze_last_scene` 1 passed). Authentic cache pairing remains
external. Offline synthetic JAG header-excludes-six + `Cache::unpack` /
`IfType::unpack` bind remain (`io::cache_289` lib tests + stage3
`offline_config_loader_synthetic_fixture`).

### Revision / adopt / logout / bothost APIs

- `Client::new` → R274; `revision()` read-only; mutation only via successful
  `adopt_from` (failed adopt leaves target unchanged, including revision).
- `from_shared` / `from_shared_with_revision` remain the host inject path
  (default 274). `cargo check -p client-play` ok. No bot action API in client.
- Public 274 `ClientProt` / `ServerProt` files untouched vs 716f79c.
- Logout opcode 121 clears stream/modals/gens; `logout` tests green.

### Outbound / enabled protocol evidence

- `p1_enc(ClientProt::*.id)` absent; draw.rs four sites use `client_opcode`.
- Verifier 0 unknown outbound lengths; 82 `production_enabled` rows.
- Stage3-corrective MESSAGE_PUBLIC / SEND_SNAPSHOT / mouse / friend oracles
  remain closed for R289, now with 274 effect isolation.
- Untracked `tools/*.py` tracers are not in the reviewed commit.

## Classification

**Offline proven (bounded):** framing/inventory (stage1); named inbound
login/widget/varp/logout/reset/rebuild; player empty/face/appearance/removal
plus remaining method128 goldens (SAY/HITMARK/ANIM/FACESQUARE/SPOTANIM/
EXACTMOVE/HITMARK2/CHAT empty); NPC empty/face/removal plus remaining
method222 goldens (HITMARK/ANIM/SAY/FACESQUARE/SPOTANIM/HITMARK2/CHANGETYPE
consume); outbound action-cut + draw remap; synthetic JAG
header-excludes-six + Cache/IfType unpack bind; 274 default construction,
public opcode tables, and sequential wave=1/scroll=2; R289 five chat
effects; renderer freeze; client-play check; contract verifier 256/82/50;
opcode 65 durable field contract.

**Failed (0030afb, preserved):** 274 `scroll:` effect 2→4 on the shared chat
path. Closed on 0e3b6a7.

**Unsupported / incomplete offline (non-blocking):** packed-chat **body**
decode beyond empty wordpack cursor proof; CHANGETYPE type-bind when
`cache.npcs` is empty; widget `gjstr` not separately psize-capped; packer
script still emits `gjstr \\x00` in a couple CASE literals while committed
manifest+tests correctly use `0x0a`; fixture `actor_update_exactmove_mask`
source_anchor cites method172 line range 8021–8054 while method128 is
5113–5254 (payload layout is still correct).

**Live-unproven (external, not this correction):** authentic 289 cache pairing
and assets/render/scene; approved endpoint/credentials; live RSA/ISAAC;
login/scene/action/logout against a real 289 server. Parent Codex owns live
coordination. Missing live/cache does not excuse available-source offline
bugs; none remaining were found that reopen t_77d35bfb or fail the
preserve-274 / NPC65 / 0x200 contract.

## Follow-up cards

None. No implementer/luna correction spawned from this re-review.
