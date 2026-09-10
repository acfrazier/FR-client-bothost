# Post-review GPU workspace gate diagnosis (t_57ec82eb)

## Scope

Orch t_95bef768 post-review workspace gate failed at HEAD
`0e3b6a719c25b134473b94023807d3c3f90e8d68` despite bounded Grok4.6 offline
acceptance t_615aa9ac. This note is the implementer diagnosis + minimal fix
receipt. Not live acceptance. Whole-branch re-review still required for the
production GPU fix.

Workspace: `/Users/acfrazier/experiments/FR-client-289`  
Branch: `codex/revision-289-client`  
CARGO_TARGET_DIR: `/Users/acfrazier/experiments/FR-client-289/target`  
Prepared base (GPU paths identical pre-fix): `716f79c`  
Render/gpu/iface test sources vs 716f79c: **no diff** before this card.

## Observed gate failures (pre-fix)

| Command | Symptom |
| --- | --- |
| `cargo test --workspace` | exit 101 at `gpu_texture::gpu_render_clamps_out_of_range_tex_id` (0 green px) or `gpu_lowmem_texture_samples_the_full_128px_layer` (seen[false;4]) |
| Isolated exact / full `gpu_texture` serial | PASS 10/10 |
| `cargo test --workspace -- --test-threads=1` | `gpu_texture` 10 PASS; `iface_model` four GPU tests FAIL with 0 overlay px |

## Root causes (evidence)

### A. iface_model four GPU failures — chrome atlas dirty-gate gap (deterministic)

Failing tests all open a **main modal after a warmup `game_draw`**:

- `gpu_draw_does_not_crash_on_mysterious_cube_modal`
- `gpu_main_modal_rect_is_opaque_over_the_scene`
- `gpu_ship_journey_paints_the_title_over_the_scene`
- `gpu_ship_journey_stays_over_a_frozen_scene`

Path:

1. `draw_scene_overlays` paints main modal into `area_game` and marks
   `overlay_coverage`.
2. `composite_scene` blits `area_game` into CPU `draw_area` at (4,4)
   (including freeze `scene_state==1`).
3. `finish` only re-uploads the chrome atlas when `chrome_upload_pending`.
4. `GpuBackend::chrome` set `atlas_dirty` for `side_modal_id` / `chat_modal_id`
   but **not** `main_modal_id` / `main_overlay_id`.

After the first frame forces `chrome_uploaded=true`, a subsequent frame that
only changes the main modal left `chrome_upload_pending=false`. The GPU frame
kept the empty scene hole → readback 0 px / black (`0x000000` vs SEA
`0x00336699`).

Diagnostic harness (temporary, removed):

| Scenario | Result |
| --- | --- |
| Modal set, **one** `game_draw` (no warmup) | PASS `rgb=0x336699` |
| Warmup `game_draw`, then modal, second draw | FAIL `rgb=0x000000` |
| Warmup + modal + `redraw_side=true` | PASS `rgb=0x336699` |

Attribution: **pre-existing on prepared GPU code** (bit-identical to 716f79c
for `gpu.rs` / `iface_model.rs` before this fix). Not introduced by 289
protocol/client commits. 289 branch only touched `draw.rs` client_opcode
sites in render/.

scene_state==1 freeze ownership was **not** changed; freeze path already
blits overlays into `draw_area`. The bug was atlas re-upload gating only.

### B. gpu_texture workspace flakes — shared model texture array race

`GpuBackend` instances share one process-wide `GpuContext` / `GpuAssets`
model texture array. `render_scene_for_test` locked assets only for
`ensure_model_textures`, then unlocked before `render_scene` + readback.
Parallel tests interleaved uploads and draws → empty/wrong layers
(0 green / seen all false).

Evidence:

- `cargo test -p client --test gpu_texture -- --test-threads=1` → stable 10 PASS
- `--test-threads=16` pre-fix → intermittent FAIL on clamps / lowmem
- Cross-binary `cargo test --workspace` multiplies contention (separate
  processes still share Metal hardware; within-process race was the
  reproducible unit-test flake)

## Minimal fixes (this card)

File: `crates/client/src/render/backend/gpu.rs`

1. **Chrome dirty gate:** force `atlas_dirty` when `main_modal_id != -1` or
   `main_overlay_id != -1` (same class as side/chat modals).
2. **Scene test lock:** process-wide `GPU_SCENE_TEST_LOCK` held for the full
   `render_scene_for_test` upload + render + readback critical section.

No pixel-oracle weakening, no silent skips, no render-ownership or
scene_state==1 freeze policy changes.

Residual note (classified, not fixed here): entity-name / cross overlays
without main_modal and without other redraw flags can still skip chrome
re-upload after warmup. Live clients usually dirty side/chat often; no
current oracle failed on that path after the main_modal fix.

## Verification (post-fix)

CARGO_TARGET_DIR=`/Users/acfrazier/experiments/FR-client-289/target`

| Command | Result |
| --- | --- |
| `cargo test -p client --test iface_model -- --test-threads=1` | 9 passed |
| `cargo test -p client --test gpu_texture -- --test-threads=16` ×5 | 10 passed each |
| `cargo test --workspace --no-fail-fast` ×2 | 0 FAILED lines |
| `cargo test -p client --test revision_289_stage1` | (bundled) green |
| `cargo test -p client --test revision_289_stage2` | 30 passed |
| `cargo test -p client --test revision_289_stage3` | 23 passed |
| `cargo test -p client --lib` | 70 passed |
| `python3 tools/verify_revision_289_contract.py` | PASS 256/82/50 |
| `cargo check -p client -p client-play` | ok |

## What this is not

- Not whole-client / live acceptance.
- Not a claim that t_615aa9ac bounded offline acceptance already covered
  workspace GPU (it did not; gate found real defects).
- Does not replace required whole-branch re-review for production GPU fix.
- Does not authorize pushes/merges/remotes/submodules/other checkouts.
