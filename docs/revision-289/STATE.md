# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Stage 3 (t_448637e1) accepted a00a641 after run496
rework. Corrective independent full-stage3 review t_5f3a92c0 REJECTED on
a00a641; correction t_c2922712 implemented (uncommitted working tree pending
reviewer). Required final branchreviewer remains behind correction approval.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stages 1–2 accepted. Corrective
t_058241ac accepted f766c9a. Stage 3 t_448637e1 rework after d441614 / run496
changes_requested (preserve rejected receipt). Independent full-stage3
corrective review: t_5f3a92c0 REJECTED. Correction: t_c2922712 (implementer).

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
  — DONE (accepted a00a641 after run496 reject on d441614; execution-lens).
- t_5f3a92c0 (reviewer): independent Grok4.5 full-stage3 corrective verdict
  — REJECTED on a00a641; see docs/revision-289/stage3-corrective-review.md.
  Preserve d441614/run496 reject receipt.
- t_c2922712 (implementer): MESSAGE_PUBLIC effects + SEND_SNAPSHOT emit +
  outbound field-oracle honesty + verifier empty-fields repair — implemented;
  awaiting reviewer.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict
  — blocked on t_c2922712 approval.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

hotspot: crates/client/src/client/client.rs — campaign cards serialized.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/82/35 after t_c2922712. Known non-blocking prerequisites remain:
authoritative game-cache pairing/manifest, approved endpoint/live authorization,
live RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99.

## Stage 2 checkpoint (t_da1f1a6a) — accepted

Commit e8ec353 (production base cc86024). Reviewer grok-4.5 round 2 approved.

## Corrective checkpoint (t_058241ac) — accepted

Commit f766c9a. Strict INV_FULL end on R289; adopt_from fail-closed; private
revision construction API.

## Stage 3 checkpoint (t_448637e1) — accepted a00a641 (execution-lens only)

### Rejected receipt (preserve)

Commit d441614 reviewed by independent Grok4.5 (run496, artifact lens):
changes_requested. Defects:

1. draw.rs bare ClientProt.id for CYCLELOGIC6/1/3 + TUT_CLICKSIDE (274 ids on
   R289 sessions).
2. protocol-289.json outbound 75× length unknown while production emit live;
   source-contract still forbade enabling unknown rows.

### Rework contents (a00a641) — closed run496 items

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

### Corrective full-stage3 review (t_5f3a92c0) — REJECTED on a00a641

Independent Grok4.5 (xai-oauth) artifact+source audit:
`docs/revision-289/stage3-corrective-review.md`. Run496 items remain closed.

### Correction t_c2922712 — implemented (pending reviewer)

Closed residuals from stage3-corrective-review.md + verifier addendum:

1. MESSAGE_PUBLIC effects in `handle_chat_input`: wave2 before wave (so
   wave2: is not swallowed), then shake/scroll/slide → effects 2/1/3/4/5.
2. SEND_SNAPSHOT (94) contract fields p8 namehash + p1 reason + p1 mute;
   client_button 601..=612 close_modal + REPORT_ABUSE emit; 613 mute toggle.
3. EVENT_MOUSE_MOVE ordered sample encodings; friend/ignore anchors method472.
4. Zero-payload outbound rows use `["(empty payload)"]`; verifier allows empty
   fields only when length==0 and still rejects empty fields when length!=0.
5. Fixture manifest completed for incomplete actor_update_face_entity_mask and
   outbound/cache doc rows so verifier structural keys pass.
6. Stage3 tests: `r289_message_public_effect_prefixes_match_java`,
   `r289_send_snapshot_report_abuse_p8_p1_p1`.

Exact tests (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage3` → 22 passed
- `cargo test -p client --lib` → 70 passed
- `cargo test -p client --test revision_289_stage1` → 26 passed
- `cargo test -p client --test revision_289_stage2` → 28 passed
- `cargo test -p client --test do_action --test walk --test prot` → 13+6+2
- `cargo test -p client --test logout --test from_shared --test player_info
  --test login_rsa --test gens --test server_packets` → 7+6+1+2+12+19
- `cargo test -p client --lib freeze_last_scene` → 1 passed
- `cargo check -p client -p client-play` → ok
- `python3 tools/verify_revision_289_contract.py` → PASS 256 inbound, 82
  outbound, 35 fixtures
- chat_mode + hud integration → 86 passed (spot)

Changed paths (working tree): client.rs; protocol-289.json; source-contract.md;
promote_outbound_289.py; verify_revision_289_contract.py; revision_289_stage3.rs;
fixtures/revision_289/manifest.json; STATE.md; tools/fix_stage3_fixtures.py.

PASS offline retained; chat effect + SEND_SNAPSHOT emit oracles green.

BLOCKED / external (not claimed):

- Authoritative 289 game-cache pairing, checksums, and real asset/render proof
- Approved live endpoint, credentials, test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Tutorial / guardian / random-event policy (explicitly not impl this milestone)

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
