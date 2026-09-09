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
complete overlay pass (after `other_overlays`). The epoch advances only when covered overlay pixels or
coverage change, so the existing upload path refreshes a moved/blinking/cleared
hint while unchanged overlays retain chrome-atlas caching. The 3D scene pixels
are excluded from the hash. The correction is limited to the overlay upload
seam; it does not force scene rebuilds, disable caching, alter packet handling,
or change CPU/GPU ownership. The scene-state-1 last-FBO freeze remains
unchanged.

## Prior verification (c9fca37; rejected as insufficient)

Round 1 rejected 0e998f2's unconditional invalidation and integer-only test.
Round 2 rejected c9fca37's evidence: its composed test manually planted overlay
pixels and called the signature itself, and the separate NPC test could skip
without cache media. These receipts are preserved below, not accepted as proof
of the production GPU call chain. Signature capture also preceded late writers.

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

## Round 3 corrective regression and verification

The sole production change relative to c9fca37 moves signature capture after
`coord_arrow` and `other_overlays`, before dropping the coverage guard. The
existing narrow signature/epoch policy and finish upload gate are unchanged.

`crates/client/src/render/backend/gpu_overlay_tests.rs` exercises the real
`draw_scene_overlays -> composite_scene -> chrome -> finish -> read_back`
chain. It creates an R289 client with a nonexistent cache path, isolated empty
Media containing two synthetic 3x3 sprites, and an in-memory NPC. No external
assets or packet data are needed. The GPU constructor must succeed (no skip).
Every overlay pixel and coverage byte is drawn by production code; the test
never calls `note_overlay_signature` or writes overlay buffers directly.

A GPU render-pass clear seeds a nonblack blue scene texture as the stable 3D
control, with `scene_ready=true`. This deliberately isolates overlay cadence
from mesh rebuild cadence; it is not a world/model rendering fixture. After
warming ordinary chrome with a hidden hint, each frame asserts clean ordinary
redraw flags and `chrome_upload_pending=false` before finish. Readback proves:

- cycle 0 visible, cycle 1 unchanged with no atlas upload;
- NPC x/z (384,1280) -> (576,1536) moves the crown immediately and clears old pixels;
- cycles 10/11 hidden, unchanged hidden frame cached, cycle 20 visible again;
- disabling the hint clears the last crown;
- a cross sprite written by `other_overlays` reaches the GPU without chrome dirtiness;
- `scene_state=1` through the actual `scene` freeze entry preserves the entire
  nonblack scene composite and scene-cycle counter, without another atlas upload.

Exact commands (serialized; all use exported
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`):

1. `cargo test -p client --all-features --lib npc_hint_production_gpu -- --nocapture --test-threads=1`
   - Initial fixture compile failed E0594 on shared `Arc<Media>` mutation;
     corrected to independently constructed Media. Preserved `target/npc-hint-round3-red.log`.
   - Before production ordering correction: exit 101, late-writer assertion
     read blue 255 instead of cyan 65535. `target/npc-hint-round3-order-red.log`.
     All preceding NPC blink/movement/clear/cache assertions had passed.
   - After moving the signature: exit 0, 1 passed, explicit GPU-executed marker
     in `target/npc-hint-round3-green.log`.
   - Mutant removing only the production signature call: exit 101, blink-on
     read blue 255 instead of magenta 16711935.
     `target/npc-hint-round3-mutant-signature.log`.
   - Independently, with signature restored, mutant removing only finish's
     epoch OR: exit 101 at the same blink-on readback assertion.
     `target/npc-hint-round3-mutant-finish.log`.
   - Both production hooks restored before final gates; no mutant retained.
2. `cargo test -p client --all-features --lib -- --test-threads=1`
   - exit 0, 80 passed, including the new GPU regression and freeze helpers;
     `target/npc-hint-round3-lib.log`.
3. `cargo test -p client --all-features --test overlays --test gpu_backend --test iface_model --test gpu_texture -- --nocapture --test-threads=1`
   - exit 0: overlays 7, gpu_backend 10, iface_model 9, gpu_texture 10;
     `target/npc-hint-round3-renderers.log`. Includes minimap-only atlas skip,
     stable present texture, frozen minimap and frozen main modal tests.
     Existing cache-conditional tests retain their inherited conditions; the
     new regression is independently cache-free and cannot silently skip GPU.
4. `cargo check -p client -p client-play --all-features` — exit 0;
   `target/npc-hint-round3-check.log`.
5. `rustfmt --edition 2021 --check crates/client/src/render/backend/gpu_overlay_tests.rs`
   and `git diff --check` — exit 0. No broad formatting change.

The full workspace gate and live performance measurement were not rerun in this
bounded corrective task. Hash collision risk and per-overlay-frame coverage scan
cost remain properties of the existing c9fca37 policy, not newly measured claims.
Same-card Grok4.5 review, corrective Grok4.6 review and fresh root live visual
verification are still required; none is replaced by the GPU fixture.

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
