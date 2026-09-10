# Revision 289 whole-branch review (t_77d35bfb)

## Verdict: REJECTED

Independent Grok 4.6 whole-branch review of `codex/revision-289-client`.
This is **not** whole-client or live acceptance. Offline stages 1–3 remain
useful bounded work; HEAD `0030afb` is **not** source-complete and **fails**
the preserve-274 requirement on public-chat effect encoding.

| Field | Value |
| --- | --- |
| Reviewer task | t_77d35bfb |
| Model / provider | grok-4.6 / xai-oauth (profile branchreviewer defaults) |
| Reviewed HEAD | `0030afbae662ee2b9939a519a23590f5c77fce3f` |
| Branch | `codex/revision-289-client` |
| Base (prepared) | `716f79c762b58daba030ceb64ba208c7f2a0ffb0` |
| Rendering base | `4f2048ea10f75b3bb92ff45610b35ba7313b0308` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| CARGO_TARGET_DIR | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by reviewer | none |
| Correction child | created with this review (see Created correction card) |

Do not treat predecessor same-card approvals as whole-branch acceptance.

## Predecessor receipts (preserved)

- Source contract d2c6318: grok-4.5 round 4 approved, bounded evidence.
- Stage 1 ad68b99 accepted.
- Stage 2 e8ec353: grok-4.5 round 2 approved; opcode 65 NPC fields left unknown.
- Corrective t_058241ac f766c9a accepted (strict inv end + session revision).
- Stage 3 t_448637e1: d441614 / run496 **changes_requested**; a00a641 execution-lens approved; run495 implementer self-review is **not** Grok4.5 acceptance.
- t_5f3a92c0 grok-4.5 **REJECTED** a00a641 (`docs/revision-289/stage3-corrective-review.md`).
- t_c2922712 grok-4.5 artifact-lens **approved** 0030afb for the four stage3-corrective defects only.

## Independent tests (this run)

No host tests. No live hosts.

| Command | Result |
| --- | --- |
| `python3 tools/verify_revision_289_contract.py` | PASS 256 inbound, 82 outbound, 35 fixtures |
| `cargo test -p client --test revision_289_stage3` | 22 passed |
| `cargo test -p client --lib` | 70 passed |
| `cargo test -p client --test revision_289_stage1` | 26 passed |
| `cargo test -p client --test revision_289_stage2` | 28 passed |
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

Native suite is green. Green tests do not prove 274 chat-effect preservation or
complete NPC field contract.

## Blocking defects

### 1. Preserve-274: MESSAGE_PUBLIC effects applied globally (0030afb)

`Client::handle_chat_input` (`client.rs` ~8684–8703) is shared. Commit 0030afb
replaced the 274/client-ts sequential prefixes:

- `wave:` → 1
- `scroll:` → **2**

with the primary **289** Java else-if table (wave2/wave/shake/scroll/slide →
2/1/3/**4**/5) on **every** revision, including default `ClientRevision::R274`.

Evidence:

- `git show 4f2048e:crates/client/src/client/client.rs` had `scroll:` → 2.
- Stage3 test `r289_message_public_effect_prefixes_match_java` only constructs
  `client_289()`. No 274 `scroll:` regression exists (`hud.rs` public-chat test
  uses `"hello"` effect 0 only).
- Opcode remap still goes through `client_opcode` (274 MESSAGE_PUBLIC id
  unchanged in `client_prot.rs`); the **effect byte** is the regression.

Required fix: revision-gate the effect table. R274 must keep sequential
`wave:`=1 / `scroll:`=2. R289 keeps the Java else-if table. Add an explicit
default-revision `scroll:` → 2 (and `wave:` → 1) production emit test. Do not
infer 274 correctness from R289 goldens.

### 2. Opcode 65 NPC fields still unknown while production decoder is live

`protocol-289.json` inbound id 65: name `server_opcode_065`, confidence
`verified-id-and-length; fields-unknown`, fields still “decode … before
implementation”. Production `dispatch_packet_289` nevertheless calls shared
274 `get_npc_pos` (`client.rs` 6697–6700).

Cold comparison of primary Java `client.java` method187/226/124/222 against
Rust (this run):

- Old-vis: 8-bit count, 1-bit info, 2-bit op, 3-bit walk / 3-bit run — match.
- New-vis: `bit_pos+21 < psize*8`, 14-bit index, sentinel 16383, 11-bit type,
  signed 5/5, 1 jump, 1 extended — match.
- Mask bits: 0x1 HITMARK2, 0x2 ANIM, 0x4 FACEENTITY, 0x8 SAY, 0x10 HITMARK,
  0x20 CHANGETYPE, 0x40 SPOTANIM, 0x80 FACESQUARE — match Java method222.
- End cursor `pos == psize` then null-list check — match.

Do **not** treat that layout match, the empty NPC fixture, or FACEENTITY golden
(`npc_info_face_entity_mask`) as complete field-contract or full-mask proof.
Stage2 review left 65 unknown; that gap is still open. Promote Java-ordered
fields into `protocol-289.json` / `source-contract.md` and add independent
source-packed goldens for remaining NPC (and remaining player) masks.

Related contract honesty: inbound 188 `player_update` lists `0x200` as
“hit/health seven fields”. Primary Java method128 `0x200` is exact-move
(4×g1 + g2 + g2 + g1); `0x400` is the second hit. Rust `EXACTMOVE=0x200`
matches Java; the JSON gloss does not.

## Cold-question results (non-blocking unless noted)

### Player masks / spawn / psize

Player method212/185/172/153/128 bit widths used by production match Java
(11-bit new-player, sentinel 2047, `bit_pos+10`, mask 0x80 → second byte).
Goldens cover empty local, FACEENTITY+APPEARANCE, truncated no-partial, and
removal. Remaining masks (SAY `~` / addChat, CHAT wordpack, HITMARK,
SPOTANIM, EXACTMOVE, HITMARK2) lack independent 289 fixtures.

HITMARK client timer is `loop_cycle+400` in shared Rust vs Java `anInt396+300`.
That is a 274-TS timer, not a wire width. Do not globally change it to 300
(would break 274). Optional R289 gate only; not this card’s 274 regression.

`read_packet` copies exactly `psize` bytes at `pos=0`. Actor decoders require
`buf.pos == size` then logout. Inventory stage1 has psize-bound / zero-publication
tests. Widget `gjstr` still reads the allocated packet buffer (Java-like), not a
separate psize cap.

### ISAAC / login

RSA plaintext order (10, four seed words, uid, user, pass) is offline-proven
(`login_rsa_plaintext_structure_ordered`). Outer wrapper 16/18|255|p2(289)|…
is proven on a loopback socket. Java `local97[i] += 50` after outbound Isaac
matches Rust `wrapping_add(50)`. The stage2 test constructs **independent**
Isaac objects to show offset ≠ identity; it does **not** prove encrypted-login
seed correspondence against a live modulus/capture. Live RSA/ISAAC remains
**live-unproven**.

### Scene / cache / freeze

Region rebuild sets `scene_state = 1` and `awaiting_player_info` (stage2
`region_scene_base_rebuild`). `check_scene` assigns `scene_state = 2` only after
location/map readiness and not while awaiting player info. Synthetic cache
does not count as scene-build proof. `freeze_last_scene` (`kind==Game &&
scene_state==1`) is unchanged vs 4f2048e. Authentic cache pairing remains
external.

### Outbound / 274 tables / scope

- Public 274 `ClientProt` / `ServerProt` files untouched in the campaign diff.
- `p1_enc(ClientProt::*.id)` absent; draw.rs four sites use `client_opcode`.
- Verifier 0 unknown outbound lengths; 82 production_enabled rows.
- Stage3-corrective MESSAGE_PUBLIC / SEND_SNAPSHOT / mouse / friend oracles
  closed **for R289** (subject to defect 1 above).
- No bot action API / host JS / bank router in client.
- Untracked `tools/*.py` tracers are not in the reviewed commit.
- Primary `client.java` hash `d03a34d8c965a426993f5a3e812bc566a80a18a91de65e994a257c49a5f5fea6` matches source-contract.

### SEND_SNAPSHOT emit

0030afb implements 601..=612 close_modal + REPORT_ABUSE p8+p1+p1 on the shared
`client_button` path (was comment-only “slice 6” at 4f2048e). Emit uses
`client_opcode`, so 274 keeps the public REPORT_ABUSE id. Not treated as the
274-effect regression; do not expand this correction into extra social work.

## Classification

**Offline proven (bounded):** framing/inventory (stage1); named inbound
login/widget/varp/logout/reset/rebuild; player empty/face/appearance/removal;
NPC empty/face/removal; outbound action-cut + draw remap; synthetic JAG
header-excludes-six + Cache/IfType unpack bind; 274 default construction and
public opcode tables; renderer freeze; client-play check; contract verifier
256/82/35.

**Failed (this HEAD):** 274 `scroll:` effect 2→4 (and added 289-only prefixes)
on the shared chat path.

**Unsupported / incomplete offline:** opcode 65 durable field contract;
full player/NPC mask goldens; protocol-289.json 0x200 gloss.

**Live-unproven (external, not this correction):** authentic 289 cache pairing
and assets/render/scene; approved endpoint/credentials; live RSA/ISAAC;
login/scene/action/logout against a real 289 server. Parent Codex owns live
coordination.

## Created correction card

- t_b04f979b (implementer): revision-gate chat effects + opcode 65 NPC contract.
  Same workspace. Must `kanban_request_review(reviewer="reviewer", ...)` then
  independent Grok4.5 same-card review, then a later Grok4.6 whole-branch
  re-review. Do not merge or push.
