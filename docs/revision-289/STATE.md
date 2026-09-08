# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Stage 3 (t_448637e1) rework after review round-1
changes; pending same-card reviewer. Corrective independent full-stage3
review t_5f3a92c0 is a prerequisite to final branchreviewer t_77d35bfb.
Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stages 1–2 accepted. Corrective
t_058241ac accepted f766c9a. Stage 3 t_448637e1 rework after d441614 / run496
changes_requested (preserve rejected receipt). Independent full-stage3
corrective review: t_5f3a92c0 (must complete before t_77d35bfb).

Serialized same-workspace dependency chain:

- t_c59985d0 (luna): source/cache inventory, complete protocol contract — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — DONE (accepted ad68b99).
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset
  — DONE (accepted e8ec353).
- t_058241ac (implementer): full inventory exact-end + session-revision API
  — DONE (accepted f766c9a).
- t_448637e1 (implementer): cache/config, basic actions and offline replay
  — rework after run496 changes_requested on d441614; pending reviewer.
- t_5f3a92c0 (reviewer): independent Grok4.5 full-stage3 corrective verdict
  — prerequisite to final branch review; preserve d441614/run496 reject.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict
  — blocked on t_5f3a92c0 (and any correction children it creates).

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

hotspot: crates/client/src/client/client.rs — campaign cards serialized.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/75/10. Known non-blocking prerequisites remain: authoritative
game-cache pairing/manifest, approved endpoint/live authorization, live
RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99.

## Stage 2 checkpoint (t_da1f1a6a) — accepted

Commit e8ec353 (production base cc86024). Reviewer grok-4.5 round 2 approved.

## Corrective checkpoint (t_058241ac) — accepted

Commit f766c9a. Strict INV_FULL end on R289; adopt_from fail-closed; private
revision construction API.

## Stage 3 checkpoint (t_448637e1) — rework after review

### Rejected receipt (preserve)

Commit d441614 reviewed by independent Grok4.5 (run496, artifact lens):
changes_requested. Defects:

1. draw.rs bare ClientProt.id for CYCLELOGIC6/1/3 + TUT_CLICKSIDE (274 ids on
   R289 sessions).
2. protocol-289.json outbound 75× length unknown while production emit live;
   source-contract still forbade enabling unknown rows.

### Rework contents (this commit)

1. `Client::client_opcode` is `pub(crate)`; draw.rs four sites route through it.
2. `protocol-289.json` outbound: all production-mapped rows have exact length +
   ordered fields + source anchors (0 unknown); OPLOC family rows added
   (method160). `source-contract.md` updated to match enabled emit.
3. Offline cache/config: `synthetic_jag` outer g3 sizes exclude the six-byte
   header; `write_synthetic_cache_dir` + tiny source-shaped flo/varp/idk +
   interface TYPE_RECT records; `load_offline_config_seam` calls production
   `Cache::unpack` / `IfType::unpack`; Client::new_with_revision binds tables.
   Authentic cache pairing still external for assets/render/scene proof only.
4. Stage3 tests: draw-path 289-vs-274 emit, CLOSE_MODAL via CLOSE_BUTTON,
   RESUME_P_COUNT via keyboard `handle_chat_input`, production cache bind.

Do not claim "every outbound site" remap without the draw coverage above;
client interact sites + the four draw sites are the production emit surface
for this stage cut.

Exact tests (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage3` → 20 passed
- `cargo test -p client --lib` → 70 passed (includes io::cache_289 ×5)
- `cargo test -p client --test revision_289_stage1` → 26 passed
- `cargo test -p client --test revision_289_stage2` → 28 passed
- `cargo test -p client --test do_action --test walk --test prot` → 13+6+2
- `cargo test -p client --test logout --test from_shared --test player_info
  --test login_rsa --test gens --test server_packets` → 7+6+1+2+12+19
- `cargo test -p client --lib freeze_last_scene` → 1 passed
- `cargo check -p client -p client-play` → ok

PASS (offline):

- R289 MOVE_GAMECLICK=234 / MINIMAP=236 / OPCLICK=67 with source payload shapes
- OPNPC2=21 len2, OPLOC1=10 len6, OPHELD1=76 len6, INV_BUTTON1=44 len6,
  IF_BUTTON=86 len2, RESUME_PAUSE=166 len2, RESUME_P_COUNT=180 len4 (keyboard),
  CLOSE_MODAL=93 len0 (CLOSE_BUTTON path), MAP_BUILD_COMPLETE=214, NO_TIMEOUT=181
- Draw paths R289: TUT_CLICKSIDE=146, CYCLELOGIC6=255, CYCLELOGIC1=130,
  CYCLELOGIC3=125 (not 274 94/188/12/52)
- R274 default emit still uses public ClientProt ids
- Offline synthetic config/interface through Cache::unpack + Client load_cache;
  scene_state stays 1 without maps
- Offline replay logout + action + inv-full + varp/widget/reset path
- protocol-289.json outbound unknown count = 0 for mapped table

BLOCKED / external (not claimed):

- Authoritative 289 game-cache pairing, checksums, and real asset/render proof
- Approved live endpoint, credentials, test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Tutorial / guardian / random-event policy (explicitly not impl this milestone)

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
