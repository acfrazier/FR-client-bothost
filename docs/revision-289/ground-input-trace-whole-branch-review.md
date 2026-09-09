# Revision 289 bounded ground-input trace: required Grok 4.6 whole-branch review

Task: `t_99cec734`
Role: required final `branchreviewer` (not a section review, not live/release authorization)
Model: grok-4.6
Provider: xai-oauth (profile defaults; no task model/provider override)
Worker session: `20260909_075729_f8ae77`
Kanban run: 967
Date (local): 2026-09-09 EDT

## Verdict: OFFLINE ACCEPTED (bounded)

Independent Grok 4.6 whole-branch review of `codex/revision-289-client` after
the bounded opt-in ground-input boundary trace. This is **not** whole-client
acceptance, **not** native ground displacement, **not** live action/logout
re-proof, **not** authentic cache pairing, **not** hardware winit cursor/scale
execution, and **not** performance acceptance.

Native ground cause remains **unknown**. Prior menu/logout proof is preserved.
This reviewer did **not** treat implementer or parent reports as evidence of
native movement.

Predecessor receipts are **preserved**, not rewritten:

- `t_77d35bfb` REJECTED `0030afb` — `docs/revision-289/branch-review.md`
- `t_615aa9ac` OFFLINE ACCEPTED (bounded) `0e3b6a7` — `docs/revision-289/branch-rereview.md`
- `t_f4e2ad64` OFFLINE ACCEPTED (bounded) `fc5516c` — `docs/revision-289/branch-final-review.md` (still untracked in this tree; identity kept)
- Cleanup A–H code `0406ceb` aggregate Grok 4.5 `e4834d7` and whole-branch Grok 4.6 `0227f3f` (`t_d1a06f94`)
- NPC overlay `1b38f18` + GPU-test portability `b03e633` + report `411ef13` (`t_e7dfd2e6`)
- Input correction `91fcadd` same-card APPROVED and whole-branch `t_7fe3337b` / docs `6475746`
- Presentation correction `03d916a` same-card APPROVED and whole-branch `t_e73008ec` / docs `171cc31`
- Diagnostic composed path `096864e` same-card APPROVED (`t_f8eda60b`)
- Parent same-card `t_2ebebe02` APPROVED `a7f4909` (actual Grok 4.5 / xai `20260909_075229_e64b14`)

Those documents are evidence. They are not a waiver to skip code.

## Frozen identity

Frozen **before** this review read production or tests:

| Field | Value |
| --- | --- |
| Branch | `codex/revision-289-client` (verified) |
| Original R274 published base | `4f2048ea10f75b3bb92ff45610b35ba7313b0308` |
| Last accepted whole-branch docs | `171cc31ab62f517cb9d51a9c77e5d012839618a1` |
| Presentation production freeze | `03d916afe29808d76709f4ff91152059da6dfcd2` |
| Native/menu/logout STATE note | `7b0ca0fc0fea79c4f3a6d34f7c7c9701e66cc945` |
| Diagnostic composed tests | `096864e367ea765c950d54f674f2321f42cff15b` |
| Parent-reviewed HEAD | `a7f49098e9bf54567807ac58d337869fd10abba0` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| `CARGO_TARGET_DIR` | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by this reviewer | none |
| Follow-up cards created | none |

Code freeze check (this run):

- `git rev-parse HEAD` equals `a7f49098e9bf54567807ac58d337869fd10abba0`.
- `git status --porcelain` has **no tracked modifications**. Inherited untracked non-deliverables left unstaged: `docs/revision-289/branch-final-review.md` and five `tools/*` tracers.
- Parent Grok 4.5 reviewed this same full hash. No source change during this review.
- `03d916a`, `171cc31`, and `4f2048e` are ancestors of HEAD.
- `git diff --stat 171cc31..HEAD` is ten files / +1201 / -2: diagnostic seams, composed tests, STATE, diagnostic report, trace report. No GPU freeze, collision, pick, or packet-algorithm files.
- `git diff 03d916a..HEAD` for `gpu.rs`, `cpu.rs`, `core/world.rs`, `dash3d/collision_map.rs`, `render/world.rs` is empty.
- `freeze_last_scene` remains `kind == Game && scene_state == 1` (`gpu.rs` 1613-1615). GPU `scene` still returns early when `kind != Game || scene_state != 2`, keeping the last FBO on scene1.

Span `4f2048e..HEAD`: 59 commits.

`docs/revision-289/STATE.md` current top still describes `t_2ebebe02` as awaiting same-card review. That lag is the implementer's `a7f4909` snapshot; this reviewer does not edit STATE. Parent `t_2ebebe02` already completed APPROVED on this hash.

Profile default verified: `~/.hermes/profiles/branchreviewer/config.yaml` `model.default: grok-4.6`, `provider: xai-oauth`. Task `model_override` / `provider_override` are null. This session is grok-4.6 / xai-oauth.

## Scope read

- Local `AGENTS.md`, `docs/revision-289/STATE.md` current top, `plan.md` native/offline acceptance, `ground-input-diagnostic-report.md`, `ground-input-trace-report.md`, `tutorial-presentation-whole-branch-review.md`.
- Production seams: `ground_trace_289.rs`, `GameShell::apply_mouse_down`, `present.rs` winit MouseInput, `Renderer::render_frame` begin/scene/post_render, `Client::doAction` WALK arm, `tryMove` emit, `game_loop` latch/consume/write, logout/lost_con/session_transfer.
- Tests: `crates/client/tests/ground_input_composed.rs` and `ground_trace_289` unit tests.

Lens: artifact-first cold read of the `03d916a..a7f4909` production diff, then comparison to the parent handoff, then independent serial execution including explicit actual GPU. Full workspace 950-test suite was **not** repeated. No native app/CUA/login/server.

## Combined trace (primary attention since `171cc31`)

### Opt-in / 274 / disabled

`GroundTrace::from_env` short-circuits on `!is_289` **before** env access. Exact `CLIENT_289_GROUND_TRACE=1` is required; unset / `0` / `true` do not construct state. R274 subprocess with env=1 emits no `ground289`. Disabled R289 composed subprocesses emit none. Construction stores a fixed-size `Option` on the existing `GameShell` owner. Disabled hot paths are `if let Some` / `.filter(active)` and do not format, allocate trace buffers, sample window size, or read the clock.

Native window construction now keeps an extra `Arc<Window>` clone so opt-in MouseInput can read inner size/scale. Sizing reads themselves run only when `ground_trace` is `Some`. That extra Arc is not a gameplay/router/pick change.

### Correlation: cursor → latch → menu → pick → route → write

1. **Native / cursor.** Winit Left/Right press samples integer cursor plus physical inner size, scale×1e6, logical size×1000 **before** `apply_mouse_down`. Host/tests without that path log `native available=0`. This review executed the compiled sample site in `present_pack` compilation/tests, **not** a live winit cursor.
2. **Latch.** `apply_mouse_down` records downs; `game_loop` `tick` compares recorded down vs latched button/x/y. Missing latch completes `no_latched_input`. Coalesced downs complete `coalesced_input`. Title-screen downs with no latch are cleared on first in-game pass.
3. **Menu.** Right-then-left without arming is one attempt. `selection` logs WALK vs menu_open only. `walk_arm` uses world click after viewport offset; GPU menu fixtures arm `(256,134)` from the saved WALK row, not the later row-click `(261,169)`. Closure without WALK completes `no_walk_selected`. Further armed click completes `superseded_input`.
4. **Render / pick.** `scene` is **pre-`backend.scene`** camera/click. `post_render` is the first armed frame's `ground_x/z`, click flag, scene state. `ground_x == -1` completes `no_pick_first_render` (first observation only; later frames are not waited). Scene1 freeze is unchanged and is not reclassified as a terrain defect.
5. **Source / route.** Consume still zeros `ground_x` then `tryMove`. Trace sets `routing` around that call only. `tryMove` emission (`length > 0`) is unchanged; the `if tryMove` split is `let moved = tryMove(...); trace; if moved { crosshair }`. Crosshair still only on success.
6. **Movement / write.** Movement fields are numeric (type, mapped opcode, length, run, abs u16 X/Z, turn count, signed deltas). `pending_write` is a diagnostic flag, not a packet. Write runs after the ordinary `stream.write` and completes `stream_write_ok` / `stream_write_error` / `no_stream_write`. Ok is API/queue only.

`seen` is a stage bitmask; `seen=63` on `no_stream_write` is not success.

### Privacy

`event` accepts only `&'static str` keys and `i64` values. Pending-message logging is `is_some() as i64`. Production trace does not read packets, ISAAC, credentials, account names, or tutorial/chat text. Composed pending control uses `PRIVATE_SENTINEL` and requires it absent from stderr.

### Bounds

64 lines including completion; 120 s waiting from first in-game tick; 10 s from first input. Expiry is cooperative (`Instant` on tick/event). Complete is idempotent. Drop emits `owner_drop` if unfinished.

## Tests actually run (this reviewer)

Serial, this checkout, `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`,
`--all-features`, `--test-threads=1`. No concurrent Cargo, no live, no 950 rerun.
Implementer RED receipts under that target directory were **read, not overwritten**.

1. `cargo test -p client --all-features --lib ground_trace_289 -- --test-threads=1 --nocapture`
   — exit 0, **4 passed**. Receipt: `target/branchreview-ground-trace-unit.log`.
2. `cargo test -p client --all-features --test ground_input_composed -- --test-threads=1 --nocapture`
   — exit 0, **6 passed / 1 ignored**. Receipt: `target/branchreview-ground-trace-composed.log`.
   Receipt subprocess covers env `1` / unset / `0` / `true`, R274, no-pick, pending, missing latch.
3. `CLIENT_289_GROUND_TRACE=1 cargo test -p client --all-features --test ground_input_composed gpu_ground_input_composed -- --ignored --exact --test-threads=1 --nocapture`
   — exit 0, **1 passed / 0 ignored** in 4.85s. Receipt: `target/branchreview-ground-trace-gpu.log`.
   Actual adapter required (`GpuBackend::try_new().expect`). Texture readback required.
   Direct and menu × reference/oblique/enclosed: six correlated observations, `native available=0`,
   enclosed `nearest=1` `abs_x=50 abs_z=50`, unblocked `abs_z=51`, all `reason=no_stream_write seen=63`.
4. `cargo test -p client --all-features --test walk --test input --test minimenu --test do_action --test game_shell --test present_pack --test render_backend --test revision_289_outbound -- --test-threads=1 --nocapture`
   — exit 0. do_action 13, game_shell 3, input 16, minimenu 27, present_pack 2, render_backend 5, outbound 27, walk 6.
   Receipt: `target/branchreview-ground-trace-affected.log`.

Scoped `rustfmt --edition 2021 --config skip_children=true --check` on
`ground_trace_289.rs`, `game_shell.rs`, `present.rs`, `renderer.rs`,
`ground_input_composed.rs`: exit 0. `git diff --check HEAD`: exit 0.

Preserved implementer REDs (not re-run as RED): `target/ground-trace-red.log`,
`target/ground-trace-expiry-red.log`, `target/ground-trace-latch-red.log`.

## Material findings (non-blocking; honest limits)

1. **Native winit cursor/scale was not executed here.** GPU/CPU fixtures log `native available=0`. Stream write Ok is not in these fixtures (`result=0` / `no_stream_write`). Do not read this review, the trace report, or synthetic bytes as native displacement.
2. **`scene` camera is pre-`backend.scene`.** After `scene()`, camera follow/jitter may differ; `post_render` does not log pick-eye camera. Native orbit diagnosis must not treat pre-scene cam as pick cam. Parent Grok 4.5 recorded the same caveat.
3. **`no_pick_first_render` is the first armed render only.** Scene1 freeze still skips mesh rebuild (`scene_state != 2`); a freeze-frame no-pick is an observation limit, not a policy change.
4. **`stream_write_error` is a production match arm** (`Some(Err) → lost_con`, same as before). Offline production-path assertion covers missing stream (`result=0`), not a forced `ClientStream` Err. Manufacturing delivery/socket success would overclaim. Root still owns any later transport evidence.
5. **`seen=63` is not success.** The no-stream GPU path has every stage bit and still completes `no_stream_write`.
6. Enclosed source-tile fallback (`nearest=1`, abs (50,50)) is primary `tryMove` behavior. Unchanged tile is not by itself proof that input or packets failed.

No gameplay/router/pick/collision/packet/lifecycle correction is justified from this offline path.

## Remaining root gates (must stay visible)

- Fresh native build after this docs commit (root-owned)
- Bounded native launch with `CLIENT_289_GROUND_TRACE=1` and a file-backed stderr receipt
- Native ground left-click / Walk-here still unresolved at tile 3094,3106
- Hardware winit cursor vs screenshot scaling
- Stream/socket delivery, server accept, player update
- Authentic 289 game-cache pairing
- Performance acceptance
- Repo hygiene (merge/push/remotes) remains parent Codex

This reviewer did not launch apps/server, touch caches/accounts/remote, edit
production/tests, or push/merge.

## Explicit offline verdict

**OFFLINE ACCEPTED (bounded)** for `codex/revision-289-client` relative to
original published base **`4f2048ea10f75b3bb92ff45610b35ba7313b0308`**, with
reviewed HEAD **`a7f49098e9bf54567807ac58d337869fd10abba0`** and last
non-diagnostic production freeze **`03d916afe29808d76709f4ff91152059da6dfcd2`**.

Meaning:

- R289-only opt-in `CLIENT_289_GROUND_TRACE=1` is diagnostic observation with
  64-line / 120s / 10s bounds and distinct absent-stage and timeout completions.
- Disabled and default 274 paths do not read the env (274) or construct/format
  (disabled 289). Scene1 last-FBO freeze is unchanged.
- Trace fields on the production path are numeric; pending-message is presence
  only. `PRIVATE_SENTINEL` is absent from composed control stderr.
- Synthetic CPU+actual-GPU composed input/menu/pick/route still emit exact R289
  MOVE_GAMECLICK bytes, including enclosed source-tile fallback. That does **not**
  accept native ground movement.
- Presentation/input/overlay evidence at `03d916a` / `91fcadd` / `1b38f18` remains.
- This does **not** authorize merge, live proof, host integration, a behavior
  fix, or whole-client / memory-campaign completion.

## Reviewer checks

- [x] Exact full hashes verified; HEAD `a7f4909` and production ancestor `03d916a` before reading code
- [x] Parent `t_2ebebe02` Grok 4.5 approval of that same hash verified
- [x] Own profile default grok-4.6 / xai-oauth verified; no card override
- [x] AGENTS, STATE current top, plan acceptance, diagnostic + trace reports read
- [x] Cold `03d916a..HEAD` production diff: opt-in, latch/menu/pick/route/write, tryMove split equivalent, freeze files untouched
- [x] Privacy: i64-only events; pending presence; no packet/account/text payload
- [x] Independent serial unit 4/4, composed 6/1 ignored, explicit GPU 1/0 ignored, affected 13+3+16+27+2+5+27+6; rustfmt/diff-check exit0
- [x] GPU log independently inspected for correlated stages, nearest1 enclosed, no_stream_write, native available=0
- [x] No source implementation edits; this file only
- [x] Offline verdict does not claim native movement, live, performance, or whole-client acceptance
