# R289 NPC hint overlay fix report

## Finding

The NPC hint crown is drawn by `Renderer::entity_overlays` into the CPU
`area_game` buffer. The GPU backend then copies the 3D scene into its scene
texture, but uploads the CPU `draw_area`/chrome texture only when side, chat,
interface, or other chrome redraw flags are dirty. NPC movement and the
`loop_cycle % 20 < 10` hint blink do not set those flags. Consequently the GPU
persistent chrome texture retains the first uploaded crown position and blink
phase, producing the observed tracking delay and frozen blink. The CPU path was
not affected because it presents the freshly composed pixels directly.

This conclusion comes from the production call flow: GPU `scene` calls
`draw_scene_overlays` -> `entity_overlays`, `composite_scene` blits
`area_game` into `draw_area`, and `finish` previously skipped the upload when
ordinary chrome was unchanged. It is not inferred from packet timing or fixture
state.

## Correction

`Renderer` now records a hash of the pixels and coverage written by the GPU
entity-overlay pass. The epoch advances only when covered overlay pixels or
coverage change, so the existing upload path refreshes a moved/blinking/cleared
hint while unchanged overlays retain chrome-atlas caching. The 3D scene pixels
are excluded from the hash. The correction is limited to the overlay upload
seam; it does not force scene rebuilds, disable caching, alter packet handling,
or change CPU/GPU ownership. The scene-state-1 last-FBO freeze remains
unchanged.

## Verification

All commands used the checkout target directory:

- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --lib render::backend::gpu::tests::overlay_signature_only_invalidates_changed_covered_pixels -- --exact` — PASS, 1 test.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --lib render::backend::gpu::tests::gpu_finish_composes_changed_overlay_and_skips_unchanged_upload -- --exact` — PASS, 1 test; composed GPU finish/readback regression proves changed overlay upload and unchanged cache.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --lib render::backend::gpu::tests:: -- --nocapture` — PASS, 7 tests.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --test overlays` — PASS, 7 tests, including production NPC hint blink/movement.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo check -p client -p client-play` — PASS.
- `git diff --check` — PASS.

`cargo fmt --all -- --check` remains nonzero because the prepared branch has
pre-existing formatting drift in reviewed files, including `client.rs` and
other unrelated modules. The changed lines were formatted manually; no broad
formatting rewrite was applied.

## Preserved invariants and limits

- CPU renderer behavior is unchanged.
- GPU scene rendering, renderer ownership, UI/chrome layering, and frame
  composition remain unchanged except for the required overlay upload trigger.
- `scene_state == 1` continues to retain the last GPU scene/FBO and does not
  rebuild the scene.
- No live server, cache, account, endpoint, or host state was accessed or
  changed.
- This is offline source/runtime-path evidence only. A fresh root-owned live
  visual run and screenshot review remain pending; source and test passes do
  not by themselves close the user-observed live defect.
