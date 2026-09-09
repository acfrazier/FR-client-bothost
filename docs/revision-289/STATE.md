# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Required Grok4.6 whole-branch review t_77d35bfb
REJECTED HEAD 0030afb — preserve docs/revision-289/branch-review.md.
Correction t_b04f979b approved 0e3b6a7. Required Grok4.6 re-review
t_615aa9ac: OFFLINE ACCEPTED (bounded) HEAD 0e3b6a7 — preserve
docs/revision-289/branch-rereview.md. GPU gate t_57ec82eb approved
fc5516c. Required final Grok4.6 re-review t_f4e2ad64: OFFLINE ACCEPTED
(bounded) HEAD fc5516c. See docs/revision-289/branch-final-review.md.
Whole client / live is not accepted.

Current cleanup checkpoint: F accepted at 97341e6 on t_4583da40 by actual
Grok4.5, after accepted E2a0f774. G implementation t_7f7981c3 is ready for
same-card review with explicit unresolved inherited fixture failures (see
cleanup-g-report.md and the final section below). H remains root-owned.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stages 1–2 accepted. Corrective
t_058241ac accepted f766c9a. Stage 3 t_448637e1 rework after d441614 / run496
changes_requested (preserve rejected receipt). Independent full-stage3
corrective review: t_5f3a92c0 REJECTED. Correction t_c2922712 approved 0030afb
(R289 oracles only). Whole-branch t_77d35bfb REJECTED on 0030afb.

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
  outbound field-oracle honesty + verifier empty-fields repair — DONE
  (approved 0030afb, R289-only goldens).
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict
  — REJECTED on 0030afb. Model grok-4.6 / xai-oauth. Deliverable
  docs/revision-289/branch-review.md. **Preserve rejection.**
- t_b04f979b (implementer): 274 chat-effect revision-gate + opcode 65 NPC
  field contract / remaining mask goldens — DONE (approved 0e3b6a7, Grok4.5
  artifact lens). Not branch acceptance.
- t_615aa9ac (branchreviewer): required final Grok4.6 whole-branch re-review
  — OFFLINE ACCEPTED (bounded) on 0e3b6a7. Model grok-4.6 / xai-oauth.
  Deliverable docs/revision-289/branch-rereview.md. Does not replace
  t_77d35bfb rejection history. Not whole-client/live acceptance.
- t_57ec82eb (implementer): post-review workspace GPU gate diagnosis + minimal
  fix (main_modal chrome dirty + scene test lock). See
  docs/revision-289/gpu-workspace-gate-diagnosis.md. Same-card Grok4.5
  approved fc5516c. Not live acceptance.
- t_f4e2ad64 (branchreviewer): required final Grok4.6 whole-branch re-review
  after GPU correction — OFFLINE ACCEPTED (bounded) on fc5516c. Model
  grok-4.6 / xai-oauth. Deliverable docs/revision-289/branch-final-review.md.
  Does not replace t_77d35bfb rejection or t_615aa9ac bounded accept.
  Not whole-client/live acceptance. Solo workspace green; concurrent
  orch maininit flake classified pre-existing /tmp, not fc5516c.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

hotspot: crates/client/src/client/client.rs — campaign cards serialized.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/82/50 after t_b04f979b fixture expansion. Known non-blocking
prerequisites remain: authoritative game-cache pairing/manifest, approved
endpoint/live authorization, live RSA/ISAAC compatibility proof. Opcode 65
inbound fields are now source-promoted (method187/226/124/222) while production
get_npc_pos remains the live decoder.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99.

## Stage 2 checkpoint (t_da1f1a6a) — accepted

Commit e8ec353 (production base cc86024). Reviewer grok-4.5 round 2 approved.
Opcode 65 NPC fields were left unknown at that time.

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

### Correction t_c2922712 — accepted 0030afb (R289 oracles; 274 chat later rejected)

Closed residuals from stage3-corrective-review.md + verifier addendum:

1. MESSAGE_PUBLIC effects in `handle_chat_input`: wave2 before wave (so
   wave2: is not swallowed), then shake/scroll/slide → effects 2/1/3/4/5.
   Applied on the **shared** path; t_77d35bfb found this breaks 274 `scroll:`=2.
2. SEND_SNAPSHOT (94) contract fields p8 namehash + p1 reason + p1 mute;
   client_button 601..=612 close_modal + REPORT_ABUSE emit; 613 mute toggle.
3. EVENT_MOUSE_MOVE ordered sample encodings; friend/ignore anchors method472.
4. Zero-payload outbound rows use `["(empty payload)"]`; verifier allows empty
   fields only when length==0 and still rejects empty fields when length!=0.
5. Fixture manifest completed for incomplete actor_update_face_entity_mask and
   outbound/cache doc rows so verifier structural keys pass.
6. Stage3 tests: `r289_message_public_effect_prefixes_match_java`,
   `r289_send_snapshot_report_abuse_p8_p1_p1`.

## Whole-branch review (t_77d35bfb) — REJECTED 0030afb (preserve)

Independent Grok4.6 / xai-oauth. Exact HEAD
0030afbae662ee2b9939a519a23590f5c77fce3f vs prepared 716f79c.
Deliverable: docs/revision-289/branch-review.md.

Blocking (addressed by t_b04f979b):

1. 274 preserve: shared handle_chat_input emitted scroll effect 4 (289 Java)
   instead of 274/client-ts 2; wave2/shake/slide added globally.
2. protocol-289.json opcode 65 still fields-unknown while production
   get_npc_pos is live; only empty/FACEENTITY NPC goldens. Player 188 0x200
   gloss said hit/health instead of exact-move.

Whole client / live acceptance is **not** complete. Missing live prerequisites
are external and are not this rejection:

- Authoritative 289 game-cache pairing, checksums, and real asset/render proof
- Approved live endpoint, credentials, test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Tutorial / guardian / random-event policy (explicitly not impl this milestone)

ISAAC +50 offset is offline-proven as a seed transform only, not encrypted-login
correspondence. Synthetic scene_state=2 is not claimed. No pushes, merges,
remotes, submodule, host, live-server, or other-checkout work.

## Correction t_b04f979b — accepted 0e3b6a7 (same-card Grok4.5)

### Changes

1. `Client::handle_chat_input` MESSAGE_PUBLIC effects gated on
   `ClientRevision`: R274 sequential wave:=1 / scroll:=2; R289 else-if
   wave2/wave/shake/scroll/slide → 2/1/3/4/5 (wave2 before wave).
2. `protocol-289.json` inbound 65 promoted to `npc_info` with method187/226/
   124/222 ordered fields + anchors; confidence `verified-length-and-fields`.
3. Inbound 188 method128 0x200 gloss corrected to exact-move (4 g1 + g2 + g2 +
   g1); 0x400 remains secondary hit. Shared HITMARK timer stays +400.
4. `source-contract.md` documents NPC 65, exact-move gloss, revision-gated
   chat effects, and +400 timer preserve.
5. Independent source-packed goldens + production tests for remaining NPC
   method222 masks (HITMARK/ANIM/SAY/FACESQUARE/SPOTANIM/HITMARK2/CHANGETYPE)
   and remaining player method128 masks (SAY/HITMARK/ANIM/FACESQUARE/SPOTANIM/
   EXACTMOVE/HITMARK2/CHAT empty). Existing FACEENTITY + R289 chat goldens kept.
6. Explicit default-revision production test
   `default_revision_message_public_scroll_wave_effects` (scroll→2, wave→1).

### Tests (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target)

- `cargo test -p client --test revision_289_stage3` → 23 passed
  (incl. default_revision_message_public_scroll_wave_effects + R289 golden)
- `cargo test -p client --test revision_289_stage2` → 30 passed
  (incl. npc_info_remaining_masks_independent +
  actor_update_remaining_masks_independent)
- `cargo test -p client --lib` → 70 passed
- `cargo test -p client --test revision_289_stage1` → 26 passed
- `cargo test -p client --test do_action --test walk --test prot --test logout
  --test from_shared --test player_info --test login_rsa --test gens
  --test server_packets` → green
- `cargo check -p client -p client-play` → ok
- `python3 tools/verify_revision_289_contract.py` → PASS 256 inbound, 82
  outbound, 50 fixtures

Same-card reviewer approved 0e3b6a7 (artifact lens). Not branch acceptance.

## Whole-branch re-review (t_615aa9ac) — OFFLINE ACCEPTED (bounded) 0e3b6a7

Independent Grok4.6 / xai-oauth. Exact HEAD
0e3b6a719c25b134473b94023807d3c3f90e8d68 vs prepared 716f79c.
Deliverable: docs/revision-289/branch-rereview.md.
t_77d35bfb rejection of 0030afb is preserved.

Closed blockers:

1. 274 preserve: handle_chat_input revision-gates MESSAGE_PUBLIC effects.
   Default Client::new sequential wave:=1 / scroll:=2. R289 else-if five
   effects retained. Explicit default_revision production test.
2. protocol-289.json opcode 65 promoted to npc_info with method187/226/124/222
   fields; remaining NPC/player mask goldens; player 188 0x200 exact-move
   gloss; HITMARK timer stays +400.

Independent tests (this run, CARGO_TARGET_DIR this checkout):

- `python3 tools/verify_revision_289_contract.py` → PASS 256/82/50
- `cargo test -p client --test revision_289_stage3` → 23 passed
- `cargo test -p client --test revision_289_stage2` → 30 passed
- `cargo test -p client --test revision_289_stage1` → 26 passed
- `cargo test -p client --lib` → 70 passed
- do_action 13, walk 6, prot 2, logout 7, from_shared 6, player_info 1,
  login_rsa 2, gens 12, server_packets 19, freeze_last_scene 1
- `cargo check -p client -p client-play` → ok

Whole client / live acceptance is **not** complete. Missing live
prerequisites remain external:

- Authoritative 289 game-cache pairing, checksums, and real asset/render proof
- Approved live endpoint, credentials, test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Tutorial / guardian / random-event policy (explicitly not impl this milestone)

ISAAC +50 offset is offline-proven as a seed transform only, not encrypted-login
correspondence. Synthetic scene_state=2 is not claimed. No follow-up
implementer/luna cards. No pushes, merges, remotes, submodule, host,
live-server, or other-checkout work.

## Post-review workspace GPU gate (t_57ec82eb)

Orch t_95bef768 gate failed workspace GPU tests on accepted HEAD 0e3b6a7.
Diagnosis + minimal production fix: see
`docs/revision-289/gpu-workspace-gate-diagnosis.md` (this card).

Root causes (not 289 protocol regressions; GPU paths were bit-identical to
prepared 716f79c before this fix):

1. `GpuBackend::chrome` omitted `main_modal_id` / `main_overlay_id` from
   atlas_dirty — post-warmup main modal left chrome_upload_pending false →
   0 overlay px on four iface_model GPU tests (deterministic).
2. Shared process `GpuAssets` model texture array raced under parallel
   `render_scene_for_test` → intermittent gpu_texture clamps/lowmem fails.

Minimal fix in `crates/client/src/render/backend/gpu.rs`: main_modal/overlay
dirty gate + `GPU_SCENE_TEST_LOCK` around scene test upload/render/readback.
scene_state==1 freeze ownership unchanged.

Post-fix verification (CARGO_TARGET_DIR this checkout):

- `cargo test -p client --test iface_model -- --test-threads=1` → 9 passed
- `cargo test -p client --test gpu_texture -- --test-threads=16` ×5 → 10/10
- `cargo test --workspace --no-fail-fast` ×2 → 0 FAILED
- revision_289 stage1/2/3 → 26+30+23; `--lib` 70; verifier PASS 256/82/50
- `cargo check -p client -p client-play` → ok

Same-card Grok4.5 approved fc5516c. Whole-branch re-review is t_f4e2ad64
below. Does not replace t_615aa9ac bounded offline receipt; not live
acceptance. Preserve branch-review.md rejection and branch-rereview.md
bounded accept.

## Whole-branch final review (t_f4e2ad64) — OFFLINE ACCEPTED (bounded) fc5516c

Independent Grok4.6 / xai-oauth. Exact HEAD
fc5516c55e9ff4b95520d39efcccda065ae7bd43 vs prepared 716f79c.
Deliverable: docs/revision-289/branch-final-review.md.
t_77d35bfb rejection of 0030afb and t_615aa9ac bounded accept of 0e3b6a7
are preserved.

GPU production fix accepted: main_modal/main_overlay chrome dirty after
warmup; test-only GPU_SCENE_TEST_LOCK. Pixel oracles unchanged vs 716f79c.
freeze_last_scene / last-FBO / minimap hold unchanged vs 4f2048e. Lazy
chrome when force cases off still proven. Baseline GPU gap attributed by
`git diff 716f79c..0e3b6a7 -- gpu.rs` empty, not by assertion.

Independent tests (this run, CARGO_TARGET_DIR this checkout):

- `python3 tools/verify_revision_289_contract.py` → PASS 256/82/50
- `cargo test -p client --test iface_model -- --test-threads=1` → 9 passed
  (ordinary + frozen main modal)
- `cargo test -p client --test gpu_texture -- --test-threads=16` → 10 passed
- revision_289 stage1/2/3 → 26+30+23; `--lib` 70
- `cargo check -p client -p client-play` → ok
- `cargo test --workspace --no-fail-fast` (solo) → exit 0, 0 FAILED

Orch concurrent workspace exit 101 on maininit (GPU green) is preserved:
fixed `/tmp/274-maininit-*` dirs, file unchanged vs 716f79c, not fc5516c.
Do not treat whole workspace as reliably green under concurrent processes.

Whole client / live acceptance is **not** complete. Missing live
prerequisites remain external. No follow-up implementer/luna cards. No
pushes, merges, remotes, submodule, host, live-server, or other-checkout
work.

## Orchestration handoff (t_95bef768)

Staged offline implementation and actual independent review sequence finished.
Final source HEAD: fc5516c55e9ff4b95520d39efcccda065ae7bd43.
Final Grok4.6 receipt: t_f4e2ad64 run 537, bounded offline acceptance;
preceding GPU correction approval: t_57ec82eb run 534.

Orch independently reproduced `cargo test --workspace --no-fail-fast` SOLO
at this HEAD: exit 0, log `target/revision-289-orch-workspace-solo.log`.
Earlier failed concurrent run remains in
`target/revision-289-orch-workspace.log`; it is not erased by the solo pass.
Contract verifier independently passed 256 inbound / 82 outbound / 50 fixtures;
stage2 30 and stage3 23 passed. `git diff --check` passed.

Parent Codex owns integration and authorization of any real 289 cache/endpoint/
test-account work. No merge/push was done. Final reviewer STATE edits and
`branch-final-review.md` are handed off uncommitted; source stays at the exact
reviewed HEAD. Untracked one-shot tracer scripts are not accepted deliverables.

Keep the bounded review limitations visible: packed-chat body and CHANGETYPE
type-binding evidence are partial; widget string reads lack a separate payload
cap; non-modal name/cross chrome refresh remains a noted limitation. These are
not claimed complete by this offline milestone. Live RSA/ISAAC, authentic cache
pairing, real scene construction and server login/action/logout remain unproven.

## Startup packet parity checkpoint (t_82d212a4) — superseded receipt

The initial opcode-13 checkpoint is retained as commit history, but its engine
comparison was superseded after correcting the source path. The authoritative
audit uses the absolute isolated engine files named in the corrective sections
below; it does not claim an engine rebuild-231 mismatch. The reproduced
`T1 - 13,3 - 219,-1` remains covered by the source-backed opcode-13 and
opcode-219 handlers. Inherited untracked review/helper artifacts remain
intentionally preserved and are not task deliverables.

## Startup packet parity correction (t_82d212a4)

Corrected the prior wrong-engine audit using the absolute isolated engine
paths. `Player.onLogin` and `ServerGameProt.ts` emit the authentic 289 startup
prefix: 219/4 rebuild, 13/3 chat filter, 235/1 friendlist status, 47/-2
ignorelist, 23/0 interface close, 120/3 PID, 172/0 var-cache reset, 75/3 or
97/6 varps, and 201/0 reset animations. Added source-backed R289 dispatch for
235, 47, 23, and 120; preserved 172 as the existing var-cache synchronization
semantics. Expanded `startup_289_source_sequence_keeps_stream_in_game` and
added `startup_289_engine_login_social_and_identity_packets_dispatch`.

Exact verification: stage2 test 33 passed / 0 failed; contract verifier PASS
256 inbound, 82 outbound, 50 fixtures; cargo check client/client-play passed;
git diff --check passed. Report corrected at
`docs/revision-289/startup-packet-parity-report.md`. Live RSA/ISAAC, authentic
cache/server pairing, script/provider-driven post-login packets, scene
readiness, and live acceptance remain external. Inherited untracked review
and helper artifacts remain preserved and unstaged.

## Startup packet parity corrective implementation (t_82d212a4)

Corrected the prior onLogin-only audit by tracing the absolute isolated engine
`Player.ts:488-533`, `NetworkPlayer.ts:286-395`, `ServerGameProt.ts:1-89`,
and `content/scripts/login_logout/login.rs2:1-116`. Added source-backed R289
dispatch for concrete login-trigger packets MESSAGE_GAME 196, CAM_RESET 133,
MINIMAP_TOGGLE 136, SET_PLAYER_OP 21, IF_SETTAB 63, UPDATE_STAT 154,
UPDATE_RUNENERGY 195, UPDATE_RUNWEIGHT 46, LAST_LOGIN_INFO 253, and first-tick
zone bootstrap 155/144/112. Existing actor/inventory handlers cover the other
concrete first-tick emissions.

The ordered regression now includes script-driven welcome/camera/minimap/player
options/tabs and stat/energy/weight/identity payloads, with exact cursors and
state assertions. The parity report was rewritten to enumerate concrete
onLogin, LOGIN-trigger, and first-tick emissions and to distinguish remaining
fail-closed non-startup packets without live-guess framing.

### Enclosed-zone correction

The R289 `UPDATE_ZONE_PARTIAL_ENCLOSED` path now translates the isolated
`ServerGameZoneProt.ts:5-14` IDs (LOC_MERGE 83, LOC_ANIM 106, OBJ_DEL 71,
OBJ_REVEAL 176, LOC_ADD_CHANGE 90, MAP_PROJANIM 87, LOC_DEL 194,
OBJ_COUNT 117, MAP_ANIM 233, OBJ_ADD 60) before invoking the shared field
decoders. A non-empty 289 frame regression proves exact consumption; unknown
inner IDs terminate at the enclosing frame boundary. The stale IF_CLOSE
comment was corrected to client.java:3195-3212, and the ordered startup test
now follows friend -> ignore -> close -> PID before varp/inventory/reset and
script/first-tick packets.

The enclosed-zone regression now uses the source-defined opcode-first framing
(90 LOC_ADD_CHANGE payload, then 71 OBJ_DEL payload), asserts the decoded zone
origin, and verifies exact outer-frame consumption.

## Startup colour corrective implementation (t_5f66df33)

Implemented source-backed R289 `IF_SETCOLOUR` opcode 160/4 after the preserved
live proof reproduced `T1 - 160,4 - 63,63`. The transitive startup trace is
`login.rs2` → `initalltabs` / `update_questlist` →
`send_quest_progress_colour` → `if_setcolour`; the report is
`docs/revision-289/startup-colour-failure-report.md`. Java field semantics are
component g2 + RGB555 colour g2 expanded to the per-client interface overlay.

Exact verification: stage2 test 36 passed, stage1 26 passed, contract verifier PASS
256/82/50, `cargo check -p client -p client-play` passed, and `git diff --check` passed.

## Full dispatch audit / cleanup contract (t_43f9ff17)

The a97b4ae audit was REJECTED by actual Grok4.5 (run840): template table
expansion did not prove semantic dispatch. Preserve that receipt; its old
22/21/213 counts and blanket completeness language are superseded, not accepted.

Corrective audit against a97b4ae is in `full-dispatch-audit.json` schema2 and
`full-dispatch-cleanup-plan.md`. Primary Java has 70 real inbound operations,
not 256: 2 complete, 41 partial, 27 missing under the stricter semantic and
publication verdicts. The other 186 byte IDs have no primary dispatch branch;
even length6 ID122 is unsupported. Each real row has fields, source/actual
Rust/274 anchors, state/generation/redraw and conservative emission evidence.
All 82 outbound operations now have emit anchors or explicit absence:
73 mapped, 1 partial, 8 missing actual emitters. Mapped does not mean live-tested.

Findings include HINT_ARROW115 exact-six including actor/stop padding; direct
zone routes and primary-only area synth91; full-zone loc expiration; missing
generation families; tutorial tab-clear sentinel; actor hit timeout/SAY;
welcome-info consume-only and tutorial/challenge chat omissions. Old outbound
contract is wrong for packed click224, camera193 field order and EMPTY232
(Java and isolated engine say0, current Rust/manifest say1). Implementation
must correct these without changing 274 behavior or weakening the verifier.

The plan partitions all real inbound operations into bounded sections plus
outbound coverage, settles bounded decode/apply/publication ownership, and
preserves per-client overlays, CPU/GPU ownership and last-FBO freeze. Concrete
engine/content witnesses include resumed tutorials, timers/queues, every
interface command edge and direct/enclosed zone emissions. No actual fixture
exclusions, code changes or new live proof are claimed.

Verification: primary branch-ID set compared to ledger (70, no missing/extra),
canonical Java lengths compared per real row (no differences), all82 outbound
names/unique IDs accounted for, source-reachability groups cover every real
inbound ID (area91 distinguished as primary-only), and section partition checked.
`python3 tools/verify_revision_289_contract.py` PASS 256/82/50 and
`git diff --check` PASS. The existing verifier checks its old manifest/fixtures,
not these newly found semantic issues; its PASS does not erase them.

Next: same-card reviewer=reviewer (Grok4.5), then root releases implementation
only after actual approval. No new cards/code/live in this audit. Authentic
server/cache pairing and later whole-branch Grok4.6/live proof remain separate
root-owned prerequisites. No merge/push/remote or host-checkout changes.

## Cleanup A implementation boundary (t_358c02ad)

203f176 and c135f60 were rejected by actual Grok4.5; retain both receipts.
The corrected implementation now has internal decoded Section-A operations,
apply-returned generation sets and explicit reset outcomes, rather than numeric
R289 publication lookup or whole-generation comparison. Generic fixed admission
includes zero; bounded strings require a terminator. Welcome uses the Java
650/655 truth table/layer and method110 closure/acknowledgement, and publishes
iface even for close-only effects. R274 dispatch/publication remains separate.

Verified in this checkout/isolated target: stage1 46, stage2 36, stage3 23,
packet7, gens12, logout7, server_packets19, with default and all features;
lib73 (including last-FBO freeze), do_action13, walk6, prot2, from_shared6,
player_info1, login_rsa2, iface_model9 and rebuild12; client/client-play
all-feature check; verifier PASS256/82/50; git diff --check. The initial
nonexistent freeze_last_scene integration-target command was corrected to its
actual lib test, not counted as a pass.

Active card remains t_358c02ad, corrective implementation ready for same-card
reviewer=reviewer / actual Grok4.5. No cleanup implementation commit is accepted
yet. Report: docs/revision-289/cleanup-a-report.md (exact commands and limitations).
Cleanup B implementation (t_36cd00a5) now includes the requested review corrections: single-modal animation resets, transformed-NPC player heads, and dedicated production-path fixtures for all 21 rows plus malformed reset and two-client overlay isolation. Report: docs/revision-289/cleanup-b-report.md. Exact isolated serial tests: client lib 73, stage1 46, stage2 40, stage3 23; focused cleanup-B 4; client check and diff check passed after the final side-icon correction. Sections C-H remain root-dispatched future work: inventory/varps/stats, actors, direct/strict zones, social/tutorial kind0, camera/audio/HUD, outbound coverage. Authentic cache/server pairing, live RSA/ISAAC/presentation and final whole-branch Grok4.6 remain separate root gates.

## Cleanup C implementation (t_19843fbb)

Section C now has bounded decode-before-apply coverage for all nine owned rows:
28, 46, 75, 76, 97, 107, 154, 172 and 195. R289 full inventory retains the
u16 count, partial inventory retains uSmart slots, extended counts are staged,
stop-transmit clears IDs without counts or side redraw, signed varps/weight
and derived stat levels are applied, varp sync reconciles without tutorial
chat redraw, changed small/large varps dirty chat, and stat-family publication
is returned for stats/energy/weight. Declared zero
inventory frames no longer use stale backing allocation bytes. Report:
docs/revision-289/cleanup-c-report.md.

Verification in isolated CARGO_TARGET_DIR: cargo check client PASS; stage1
46/46; stage2 42/42; server_packets 19/19; focused cleanup C 2/2. The
combined stage1 serial load-sensitive fixed-empty-sync failure was reproduced
as pre-existing and passed isolated. Same-card reviewer=reviewer is next;
Sections D-H and live/cache/Grok4.6 gates remain separate.

## Cleanup D implementation (t_115f2843)

Accepted C foundation: 4a2f5aa (t_19843fbb, actual Grok4.5 approved).
Active D card t_115f2843 implements IDs65/188/201 through private staged actor
decode, packet-free apply and operation-returned publication. Full movement,
mask and appearance frames validate before semantic mutation. Actor identity,
local ownership and R274 behavior remain intact; R289 hit timers are +300,
SAY/public chat publish only actual log changes, reset-anims publishes both
actor families. Report: cleanup-d-report.md. Pending same-card Grok4.5 review;
no D commit is accepted yet.

Verification: actor suite13; full native workspace all-features PASS, including
lib76, stage1 46, stage2 42, stage3 23, server_packets19 and client-play4;
workspace all-feature check, new-file rustfmt, contract verifier256/82/50 and
diff check PASS. Preserve first workspace failure in target/cleanup-d-workspace.log:
old stage2 +400 expectations corrected to primary +300; known stage1 sync
intermittent failed once, passed subsequent full run. Final passing output is
target/cleanup-d-all-features.log. No test result implies live compatibility.

Next: same-card reviewer=reviewer, then root decides E-H. Authentic cache/server
pairing, live RSA/ISAAC/presentation and required final whole-branch Grok4.6
remain root gates. No host/server/live or renderer ownership changes.

## Cleanup E implementation (t_3aa4481e)

Accepted D foundation: 5f5148b (t_115f2843, actual Grok4.5 approved).
E stages all eleven primary zone operations in direct and enclosed paths;
entire enclosed frames validate before origin/effects/publication. Adds
area-synth91, full-zone pending-loc expiry, primary LOC_ANIM bounds/heights,
typed rebuild application and coalesced scene/player merge publication.
R274 and rendering/cache ownership remain intact. Independent production
fixtures and source anchors: cleanup-e-report.md. E is not yet accepted;
same-card reviewer=reviewer is next.

Final verification: zones8, full native workspace and workspace all-features
PASS (including actors13, stage1 46, stage2 42, stage3 23, server_packets19,
legacy zone45 and client-play4). All-feature check, new-file rustfmt,
contract verifier256/82/50 and diff check PASS. Exact commands and failed
TDD/early stage1 runs are preserved in cleanup-e-report.md; passing logs:
target/cleanup-e-workspace-final.log and target/cleanup-e-all-features-final.log.
Earlier native run timed out at180s; earlier all-features run had transient
stage1 sync/welcome failures, both passing in final full runs without timing
test edits. Offline gates do not establish live compatibility.

Next: same-card Grok4.5 review of E; root then decides F-H. Authentic
cache/server pairing, live RSA/ISAAC/presentation and final whole-branch
Grok4.6 remain required. Hotspot client.rs remains serialized. No push,
merge, remote/live, host/server changes or F-H implementation.

## Cleanup F implementation (t_4583da40)

Accepted E foundation: 2a0f774 (t_3aa4481e, actual Grok4.5 approved).
F stages social/chat/player-option rows 13, 21, 47, 168, 196, 235 and 243
through bounded R289 decode, packet-free apply and outcome publication. It
adds strict ignore-list framing/capacity validation, private-message short
header and exact-body staging, friend notice publication only on actual chat
changes, all trade/duel/challenge request suffixes, tutorial kind-0 storage
and click clearing, null/index-safe player options, and declared chat filter
publication with redraw flags. R274 behavior and public constants remain
unchanged. Report: cleanup-f-report.md. Same-card reviewer=reviewer is next.

Verification: client check PASS; stage2 42/42; client lib/tests PASS including
zone45; workspace all-features PASS including stage1 46, stage2 42, stage3 23,
server_packets19, zones8, legacy zone45, renderer suites and client-play4;
contract verifier PASS 256/82/50; diff check PASS. This is offline proof only:
authentic cache/server pairing, live RSA/ISAAC/presentation and required final
whole-branch Grok4.6 remain root gates. Hotspot client.rs remains serialized;
no host/server, renderer ownership, remote, merge or push changes.

Review correction in progress: chalreq now extracts the Java-defined body;
tutorial kind0 renders message plus `Click to continue` and click dismissal
clears the stored message while retaining the tutorial interface. Added
production-path stage2 fixtures for those behaviors, ignore remainder/capacity,
private short header, ignored exact consumption, all request suffixes and
player-option null/OOR edges. Focused cleanup-F tests pass 3/3; full affected
suites and same-card reviewer remain next.

Follow-up review correction added production-path stage2 coverage for private
deduplication, a nonzero ignored private ID with WordPack body and exact frame
consumption, staff levels 0/1/2/3, UPDATE_FRIENDLIST add/same-world/login/
logout/current-world sorting and actual notice generations, SET_PLAYER_OP index
5/nonzero priority, and malformed later fields. Focused cleanup-F tests pass
6/6; full stage2, client lib/tests, workspace all-features, client-play
check, contract verifier and diff check pass. Ready for same-card reviewer handoff.

## Cleanup G implementation (t_7f7981c3)

Accepted F foundation: 97341e64aea648b1d92d7f2a10e60080ff682f17,
actual Grok4.5 review on t_4583da40. G stages all twelve owned rows
29/73/82/115/133/136/164/177/187/204/208/247 through bounded R289Operation
decode, packet-free apply and coalesced publication in private misc_289.
Camera, six-byte unsigned hints, global sound/music gates, flag/timer/minimap
and multiway HUD behavior have production-path fixtures. R274 constants/default
and renderer resource/last-FBO ownership remain unchanged. Report:
cleanup-g-report.md. G is not accepted; same-card reviewer=reviewer is next.

Verification in this checkout's CARGO_TARGET_DIR: default focused G10/10,
iface_model9/9, gpu_texture10/10 and lib73/73 pass; all-feature workspace check,
contract verifier256/82/50, new-file rustfmt and diff check pass. Prior native
workspace passed (199.3s receipt, target/cleanup-g-workspace.log). Earlier
all-feature run was interrupted exit130/158.4s, not a pass. Preserve it and the
prior resume log with stage1 welcome/logout, stage2 and E zone failures.

Fresh completed solo all-feature workspace run is NOT GREEN:
target/cleanup-g-all-features-final.log ends with two failed targets. Stage2
47/48 rejects the enclosed loc fixture; E zones4/8 rejects loc/object/spotanim
config IDs. These fixtures use /tmp without populating their required tables;
their code and production E validators are unchanged from accepted F. No cache
seeding, weakened bounds or E test edits were made in G. Source attribution is
not a separately executed baseline build. Final stage1 46/46, G10/10, lib76,
stage3 23, GPU suites and client-play4 passed. Exact failures and commands are
in the report; reviewer/root must retain this regression-gate limitation.

No H, live, host/server/cache, remote, merge, push or new-card work. Root retains
authentic cache/server pairing, live RSA/ISAAC/presentation, resolution of the
inherited fixture gate and required final whole-branch Grok4.6. Synthetic pixels
and audio request state are not real presentation/audible proof. Hotspot:
crates/client/src/client/client.rs remains serialized; inherited untracked
review/helper artifacts remain unstaged.
