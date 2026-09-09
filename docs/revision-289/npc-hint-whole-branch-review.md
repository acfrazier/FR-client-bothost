# Revision 289 NPC hint overlay: required Grok 4.6 whole-branch review

Task: `t_e7dfd2e6`
Role: required final `branchreviewer` (not a section review, not live/release authorization)
Model: grok-4.6
Provider: xai-oauth (profile defaults; no task model/provider override)
Worker session: `20260908_230411_5b9c2c`
Date (local): 2026-09-08 EDT

## Verdict: OFFLINE ACCEPTED (bounded)

Independent Grok 4.6 whole-branch review of `codex/revision-289-client` after
cleanup A–H and the combined NPC hint overlay correction. This is **not**
whole-client acceptance, **not** live visual/action/logout proof, **not**
authentic cache pairing, and **not** performance acceptance. The earlier live
tutorial-room run that showed a frozen/laggy NPC arrow remains an open
root-owned visual gate.

Predecessor receipts are **preserved**, not rewritten:

- `t_77d35bfb` REJECTED `0030afb` — `docs/revision-289/branch-review.md`
- `t_615aa9ac` OFFLINE ACCEPTED (bounded) `0e3b6a7` — `docs/revision-289/branch-rereview.md`
- `t_f4e2ad64` OFFLINE ACCEPTED (bounded) `fc5516c` — `docs/revision-289/branch-final-review.md` (still untracked in this tree; identity kept)
- Cleanup A–H code `0406ceb` aggregate Grok 4.5 `e4834d7` and whole-branch Grok 4.6 `0227f3f` (`t_d1a06f94` actual `20260908_221302_f73107`)
- Same-card `t_1ced5ad3` round 1/2 REJECTED `0e998f2` / `c9fca37` (broad invalidation / incomplete proof)
- Same-card `t_1ced5ad3` round 3 APPROVED `1b38f18` (actual Grok 4.5 / xai `20260908_225610_906ad2`)
- Parent `t_e042ff2a` APPROVED test-portability `b03e633` (actual Grok 4.5 / xai `20260908_230211_a86f50`)

Those documents are evidence. They are not a waiver to skip code.

## Frozen identity

| Field | Value |
| --- | --- |
| Branch | `codex/revision-289-client` (verified) |
| Original R274 published base | `4f2048ea10f75b3bb92ff45610b35ba7313b0308` |
| Cleanup production code | `0406ceb56aea76e2d4474bbd54f8441f9b0f5245` |
| Cleanup whole-branch report | `0227f3f5e106e37e9b60bff44a35bf9f89ca9daa` |
| Combined overlay production | `0e998f2` (rejected) + `c9fca37` (rejected) + `1b38f18` (approved) |
| Portability HEAD under review | `b03e633ffdcbb4c2903b6b569497aa3d4b7b9a00` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| `CARGO_TARGET_DIR` | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by this reviewer | none |
| Follow-up cards created | none |

Code freeze check (this run):

- `git rev-parse HEAD` equals `b03e633ffdcbb4c2903b6b569497aa3d4b7b9a00`.
- `git diff --name-only 0406ceb HEAD -- ':!docs' ':!*.md'` is four overlay files only: `gpu.rs`, `gpu_overlay_tests.rs`, `renderer.rs`, `crates/client/tests/overlays.rs`.
- `git diff --stat 0406ceb HEAD -- crates/client/src/io crates/client/src/client crates/client/src/prot protocol-289.json` is empty. No protocol/lifecycle/config change after cleanup.
- `git diff --stat 0406ceb HEAD -- crates/client/src/render/draw.rs` is empty. The rejected `0e998f2` unconditional `overlay_epoch += 1` in `entity_overlays` was removed by `c9fca37` and did not return.
- Working tree has no tracked modifications. Inherited untracked non-deliverables left unstaged: `docs/revision-289/branch-final-review.md` and five `tools/*` tracers.

Span `4f2048e..HEAD`: 51 commits.

## Scope read

- Local `AGENTS.md`, `docs/revision-289/STATE.md` current top, `full-dispatch-cleanup-plan.md` decision 8 (CPU/GPU ownership, last-FBO freeze, no bot action API), cleanup aggregate + whole-branch reports, `npc-hint-overlay-fix-report.md`.
- Production overlay seam: `GpuBackend::draw_scene_overlays` → `composite_scene` → `chrome` → `finish` upload gate; `Renderer::note_overlay_signature`; `freeze_last_scene` / `should_cls_scene_overlays`; ordinary chrome dirty flags including `main_modal_id` / `main_overlay_id`; minimap live/held.
- Tests inspected as behavior oracles: production GPU fixture (hard adapter fail), integer epoch helper, planted-pixel hasher unit test (not accepted as production proof), cache-conditional CPU overlay test (not accepted as GPU proof).

Lens: artifact-first cold read of overlay ownership/draw coverage/signature order/finish refresh, then comparison to handoff claims, then focused independent execution. Full workspace 950-test suite was **not** repeated; H receipt was recounted.

## Combined overlay correction (primary attention)

### Defect (source path, not packet inference)

GPU `scene` draws CPU overlays into `area_game` (`entity_overlays` writes the
NPC hint crown from `loop_cycle % 20 < 10` and current entity x/z).
`composite_scene` blits that into `draw_area`. `finish` uploads the persistent
chrome texture only when ordinary chrome is dirty **or** the overlay epoch
changed. Before the correction, NPC motion and blink did not set chrome flags,
so the GPU atlas kept the first crown.

CPU present-from-pixels was unaffected. This review does not treat that as
closing the live GPU defect.

### Rejected history (retained)

1. `0e998f2` advanced `overlay_epoch` on every `entity_overlays` call. That is
   unbounded overlay-driven atlas refresh, not change-gated caching. Round 1
   rejected it.
2. `c9fca37` hashed covered pixels and gated finish on epoch inequality, but
   captured the signature **before** `coord_arrow` / `other_overlays`, and its
   GPU evidence planted pixels / called `note_overlay_signature` itself. Round 2
   rejected incomplete production-chain proof. Order-red receipt
   `target/npc-hint-round3-order-red.log` still shows late-writer readback
   **blue 255** instead of cyan **65535**.
3. `1b38f18` moved capture to after `other_overlays` and added
   `npc_hint_production_gpu_*`, which uses the real compositor chain. Round 3
   approved that production path.
4. `b03e633` is attribute-only `#[ignore]` on the GPU-required test plus docs.
   Runtime body and hard `try_new().expect("GPU required for overlay proof")`
   remain. Parent `t_e042ff2a` approved the portability follow-up. This review
   does **not** count ignored/filtered tests as GPU proof.

### Restored production hooks (this HEAD)

`draw_scene_overlays` (`gpu.rs`):

1. optional overlay `cls` unless `freeze_last_scene`
2. zero coverage, install `coverage_guard`
3. `entity_overlays` → `coord_arrow` → `other_overlays`
4. `note_overlay_signature(&self.overlay_coverage)` **before** dropping the guard
5. drop guard

`note_overlay_signature` hashes only covered (`mask != 0`) pixels plus coverage
bytes. Uncovered 3D pixels are excluded. Epoch advances only on signature
change (`wrapping_add(1)`).

`finish` ORs `overlay_upload_needed(overlay_uploaded_epoch, r.overlay_epoch)`
into `chrome_upload_pending`. Upload then copies `overlay_uploaded_epoch`.
Unchanged overlays keep the existing atlas. Ordinary chrome still requires
side/chat/icons/modal dirtiness independently of this OR.

Mutation logs (retained, not re-mutated this run):

- Remove production signature **or** finish epoch OR → blink-on read **blue 255**
  instead of magenta **16711935** (`npc-hint-round3-mutant-signature.log`,
  `npc-hint-round3-mutant-finish.log`).
- Capture before late writers → cross read **blue 255** instead of cyan
  **65535** (`npc-hint-round3-order-red.log`).

Both production hooks are present at this HEAD. No mutant remains in source.

### What this is not

- Not `redraw_all`, not a per-frame atlas upload, not a memory/per-bot
  retention store (one `u64` epoch + optional signature on `Renderer`).
- Not a protocol, packet, ISAAC, cache-unpack, or outbound change.
- Not a CPU renderer ownership transfer. CPU `cpu.rs` overlay order is
  unchanged.
- Not a last-FBO policy change: `freeze_last_scene` is still
  `kind == FrameKind::Game && scene_state == 1`. Freeze entry in `scene()`
  still calls `draw_scene_overlays` without mesh rebuild / `scene_cycle++`.
  `composite_scene` freeze still blits last `area_game` and returns.
  Minimap remains live only at `scene_state == 2`; freeze reuses held texture.

## R274 preserve / cleanup contract carry-forward

Independently re-checked against overlay delta, not re-litigated as new
protocol work:

- Default construction remains R274 (`io::revision::tests::default_revision_is_274_and_keeps_public_table` passed in this run's `--lib`).
- Public 274 tables / no bot action API: overlay commits do not touch
  `client.rs` protocol dispatch.
- Cleanup 70/82 contract at `0406ceb` is unchanged by this delta. Prior
  whole-branch bounded accept `0227f3f` remains the cleanup receipt.
- `main_modal_id` / `main_overlay_id` chrome dirty from `fc5516c` is still
  present. Overlay epoch is an additional upload trigger, not a replacement
  of that gate.
- Dead dual-path arms / assert-label / opcode-232 field-style residuals from
  cleanup whole-branch remain non-blocking maintenance debt.

## Independent verification (this run)

cwd `/Users/acfrazier/experiments/FR-client-289`,
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`, serial,
no overlapping Cargo, no retry-until-green, no assertion edits, no source
implementation edits.

| Command | Result |
| --- | --- |
| `cargo test -p client --all-features --lib npc_hint_production_gpu -- --test-threads=1 --nocapture` (no `--ignored`) | exit 0; **0 passed, 1 ignored**; ignore reason present; **no** GPU execution sentinel |
| `cargo test -p client --all-features --lib npc_hint_production_gpu -- --ignored --nocapture --test-threads=1` | exit 0; **1 passed, 0 ignored**; sentinel `GPU overlay proof executed: blink, x/z movement, clear, cache, late writer, last-FBO freeze` |
| `cargo test -p client --all-features --lib -- --test-threads=1` | exit 0; **79 passed, 1 ignored** (the GPU-required proof). Includes freeze helpers, overlay epoch/signature unit tests, and skippable planted-pixel GPU finish helper which **did run** here (`ok`) but is still not the production proof |
| Recount `target/cleanup-h-all-features-final.json` + `.log` | returncode 0; 188.57s; 72 `test result: ok`; **950** passed; 0 FAILED summaries |
| Preserve prior failure | `target/cleanup-h-workspace.log` still contains `welcome_then_logout` FAILED (exit-101 era) |
| Prior renderer integration receipt | `target/npc-hint-round3-renderers.log`: gpu_backend 10 + gpu_texture 10 + iface_model 9 + overlays 7 = **36** passed |
| Prior `cargo check -p client -p client-play --all-features` | `target/npc-hint-round3-check.log` Finished ok |
| `rustfmt --edition 2021 --check gpu_overlay_tests.rs` | exit 0 |
| `git diff --check 0406ceb HEAD` overlay sources | clean |

Ignored/filtered tests are **not** GPU proof. The explicit `--ignored`
invocation is the GPU proof for this review.

The production fixture (inspected, then executed): R289 client, nonexistent
cache path, isolated empty Media with synthetic 3×3 crown/cross, in-memory NPC,
nonblack blue 3D texture with `scene_ready=true`, warm hidden hint, then
`draw_scene_overlays → composite_scene → chrome → finish/read_back` with clean
ordinary chrome flags. Asserts blink on/off, x/z 384,1280 → 576,1536 with old
pixel clear, unchanged-frame upload count, other_overlays cross, and
`scene_state=1` freeze retaining nonblack composite without `scene_cycle`
bump or extra atlas upload. Adapter failure is `expect` panic, not skip.

## Findings

### Blocking (must fix before this offline whole-branch accept)

**None identified** against the frozen overlay + cleanup branch at HEAD
`b03e633ffdcbb4c2903b6b569497aa3d4b7b9a00`.

### Non-blocking (root may defer; independently assessed)

1. **Stale `note_overlay_signature` doc comment** still says it is called after
   `entity_overlays`. Production call is after `coord_arrow` and
   `other_overlays`. Comment debt only; behavior is the late-writer capture.
2. **`gpu_finish_composes_changed_overlay_and_skips_unchanged_upload`** can
   still `return` on missing adapter. It is not the production proof. The
   ignored `npc_hint_production_gpu_*` test remains hard-fail when invoked.
3. **`overlays.rs` NPC blink/move test** still silent-returns without a media
   cache file. CPU-path convenience only; not GPU proof.
4. **Cleanup residuals** (dead `dispatch_packet_289` fallback arms, “Section A”
   assert label, opcode 232 empty-fields style) remain as previously judged
   currently safe / cosmetic.
5. **Hash collision / per-frame covered-pixel scan cost** are properties of the
   `c9fca37` signature policy and were not newly measured. Not a correctness
   blocker for this offline gate.
6. **Inherited untracked tracers** and untracked `branch-final-review.md` left
   unstaged.

### Remaining root gates (must stay visible)

- Fresh native build after this docs commit (root-owned)
- Live visual recheck of the NPC hint (root-owned; prior tutorial-room run
  reported frozen/laggy arrow; owned client stopped)
- Live action / logout acceptance
- Authentic 289 game-cache pairing, checksums, real asset/render proof
- Approved live endpoint / credentials / test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Performance / lag acceptance of the overlay hash+upload path
- Tutorial / guardian / random-event policy (explicitly out of this milestone)
- Repo hygiene (merge/push/remotes) remains parent Codex

Offline acceptance of this branch does **not** establish those. This reviewer
had no server/network/CUA/live/remote/account/cache/config access.

## Explicit offline verdict

**OFFLINE ACCEPTED (bounded)** for `codex/revision-289-client` relative to
original published base **`4f2048ea10f75b3bb92ff45610b35ba7313b0308`**, with
cleanup production frozen at **`0406ceb56aea76e2d4474bbd54f8441f9b0f5245`**
and overlay/portability HEAD **`b03e633ffdcbb4c2903b6b569497aa3d4b7b9a00`**.

Meaning:

- Combined overlay ownership is change-gated signature + finish epoch OR, not
  redraw-all, after the complete overlay pass including late writers.
- Independent explicit GPU proof executed 1 pass / 0 ignored with the required
  sentinel. Default `--lib` is 79 pass / 1 ignored and is not GPU proof.
- R274 defaults, last-FBO `scene_state==1`, ordinary chrome/minimap cache, and
  cleanup protocol surface are preserved by this delta.
- Prior rejections (`0030afb` whole-branch, overlay rounds 1–2) remain history.
- This does **not** authorize merge, live proof, host integration, performance
  claims, or whole-client acceptance. It does **not** claim the user-observed
  live NPC arrow defect is visually closed.

## Reviewer checks

- [x] Exact full hashes verified; overlay production + ignore attribute at `b03e633`
- [x] AGENTS, STATE current top, cleanup plan decision 8, aggregate + whole-branch reports, overlay fix report read
- [x] Combined overlay draw coverage, signature capture order, finish texture refresh inspected
- [x] `0e998f2` broad invalidation absent at HEAD; `c9fca37` early-capture defect corrected
- [x] Source hooks restored; mutant/order-red receipts retained with expected blue-vs-magenta/cyan
- [x] R274 defaults, last-FBO, ordinary chrome/minimap, no protocol change after `0406ceb`
- [x] Explicit `--ignored` GPU proof 1/0 + sentinel; default lib 79/1; default filter 0/1 no sentinel
- [x] H 950-test receipt recounted including original failed suite
- [x] No source implementation edits; this file only
- [x] Offline verdict does not claim live/performance/whole-client acceptance
