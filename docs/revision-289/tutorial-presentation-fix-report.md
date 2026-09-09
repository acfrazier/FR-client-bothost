# Tutorial pending-message presentation correction

Task: t_ea8cdf17. Implementation boundary: 2026-09-09 10:55 UTC.
Branch: `codex/revision-289-client`.
Starting HEAD: `647574624ab0908e2b58d45821082c78236ce6a6`.
Verified parent production: `91fcadde87d9566f7844de4d18b8823fa8dd09fe`;
only the final whole-branch report differs between that source and starting HEAD.
Parent t_7fe3337b completed OFFLINE ACCEPTED (bounded), expressly excluding
pending-message presentation. Base274: `4f2048ea10f75b3bb92ff45610b35ba7313b0308`.

## Primary mapping and implementation

Read-only primary: `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/client.java`.
No primary source material or cache is copied into the fixture.

- method129 J5266 draws chatback first. J5267-5272 gives social prompt then
  amount prompt priority. J5273-5275 selects a non-null pending message before
  chat modal J5276-5277 and tutorial/plain chat J5278 onward.
- Message is centered at (239,40), colour 0; `Click to continue` is centered
  at (239,60), colour 128. Empty non-null message still selects the cue.
- `draw_chat` now implements that branch before either interface, R289-only.
  It draws directly onto chatback, not over a tutorial interface, even if the
  tutorial is closed or a chat modal is open. Social/amount priority stays.
- J9832-9840 requests chat redraw on every pending frame, then clears the
  flag after drawing. `CpuBackend::begin` publishes only `redraw_chat` for
  R289 + Some, including empty. Existing `chrome` draws/clears it normally.

### Why the shared begin stage

`Renderer::game_draw` runs begin -> scene -> composite_scene -> chrome ->
finish. GPU begin already delegates to CPU begin (`gpu.rs` 1188). GPU chrome
captures `redraw_chat` before delegating to CPU chrome (`gpu.rs` 1395-1400).
Putting this primary chat flag only inside CPU chrome would miss that GPU
capture when no unrelated atlas trigger is set. Publishing in shared begin
keeps the ordinary existing upload propagation without changing GPU code,
forcing redraw-all, invalidating scene/overlay signatures, or adding a new
atlas policy. Primary's late dirty assignment has the same chat effect here;
no stage between begin and chrome clears chat dirtiness.

Scene-state1 last-FBO and NPC overlay signature logic are untouched. Tests
exercise the CPU loading-frame path and verify unchanged scene/non-chat
pixels; this is not a fresh hardware GPU freeze or upload-count proof.
GPU propagation above is a source-path argument, not an executed GPU claim.
No adapter-dependent test was necessary to change this existing shared flag
path; the existing ignored-by-default GPU-only convention is unchanged.

### Deliberate 274 behavior

Read `draw_chat` from the pinned base: social -> amount -> chat modal ->
tutorial interface -> ordinary chat, with no pending-message branch. The new
branch and new per-frame dirty condition are both revision-gated. Removing the
cleanup-era overlay also restores base behavior for an artificially injected
274 pending field (normal 274 capture never sets one). Base interface ordering
and pending-free dirtiness are preserved, not the erroneous cleanup overlay.
Approved nullable input/capture/ack code and NPC overlay files are unchanged.

## Actual production regression path

`crates/client/tests/tutorial_presentation.rs` uses `Renderer::new(false)` and
real `game_draw`, normal CpuBackend scheduling, production `draw_chat`,
`draw_interface`, PixFont glyph rasterization and the chat blit into draw_area.
Every chat-surface pixel and every corresponding composite pixel is checked.
No helper selects a branch on behalf of production and no source-text test is
used. The expected prompt pixels do not call production font layout/drawing.

Fixtures are independently authored: a solid nonzero indexed chatback, two
solid distinguishable interface rectangles, one-pixel glyph masks with advance
2 and baseline height1, and a nonblack stable scene marker. PixFont fields and
fresh Media are seeded directly; no title pack, local font or cache gate can
silently return success. The absent cache path is asserted absent. The
fixture preallocates chat/game surfaces to retain its synthetic media through
normal prepare_game, then warms the initial redraw flags before assertions.

Five focused tests cover:

1. Nonempty pending replaces the tutorial rectangle with exactly chatback,
   black message and DARKBLUE cue at primary-centered positions.
2. Repeated pending frames with incoming redraw_chat false, unchanged text,
   replacement text and empty text repair a deliberately poisoned chat
   surface. All ordinary redraw flags are initially clear; scene and every
   non-chat composite pixel remain unchanged. Backend clears the chat flag.
3. Empty/nonempty across tutorial open, closed, chat-modal open, and both
   interfaces open. Pending suppresses the underlying interface/ordinary chat.
   A real shell LEFT through the production pre-menu handler subsequence
   clears pending, dirties chat and emits no action. The next game_draw restores
   the exact prior underlying pixels; the following LEFT dispatches synthetic
   side-widget IF_BUTTON `[86,0,31]` (J11234-11242). This is a bounded input
   subsequence, not a full game_loop or native session.
4. Social outranks amount and pending; amount outranks pending, with exact
   independently expected prompt pixels in both revisions.
5. R274 preserves base modal/tutorial/chat ordering and pixels for injected
   Some(nonempty/empty), and does not redraw a poisoned chat surface when its
   ordinary flag is clear.

## Preserved RED/mutation evidence and exact commands

All commands ran serially, in this checkout, with
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`.
Logs are retained in that target directory; no old campaign receipts replaced.

Initial command:
`cargo test -p client --all-features --test tutorial_presentation -- --test-threads=1`

- `tutorial-presentation-red-precedence.log`, exit101: fixture compile errors
  (usize interface slot API, PixMap pixels field, immutable Arc media).
- `tutorial-presentation-red-behavior.log`, exit101: fixture attempted get_mut
  on shared default Media. Repaired with independently owned `Media::empty`.
  Neither of these is counted as behavioral RED.
- `tutorial-presentation-red-pixels.log`, exit101: actual old-production RED,
  tutorial pixel (0,0) =1193046 versus chatback11256099. 0 passed/1 failed.
- `tutorial-presentation-green-precedence.log`, exit0: 1 passed after branch
  correction; no dirty change yet.

Dirty tracer command:
`cargo test -p client --all-features --test tutorial_presentation pending_redraws_each_frame_without_other_dirty_regions -- --exact --test-threads=1`

- `tutorial-presentation-red-dirty.log`, exit101: missing primary scheduling
  leaves poisoned (0,0)=15658734 instead of11256099. 0 passed/1 failed.
- Full focused command after shared begin correction:
  `tutorial-presentation-green-dirty.log`, exit0, 2 passed.
- Expanded matrix, same full focused command:
  `tutorial-presentation-matrix.log`, exit0, 5 passed.

Independent colour mutation command:
`cargo test -p client --all-features --test tutorial_presentation pending_replaces_tutorial_on_chatback_with_primary_colours -- --exact --test-threads=1`

- Changed only the continue cue back to BLACK, retaining corrected precedence
  and scheduling. `tutorial-presentation-red-colour-mutation.log`, exit101:
  actual cue pixel (222,59)=0 versus128. 0 passed/1 failed.
- Mutation restored to DARKBLUE before final regression; it is not in the diff.

Final affected regression command:
`cargo test -p client --all-features --test tutorial_presentation --test hud --test input --test revision_289_stage2 --test revision_289_stage3 --test revision_289_outbound --test minimenu --test chat_mode -- --test-threads=1`

`target/tutorial-presentation-regression.log`, exit0:
chat_mode4, hud86, input16, minimenu27, outbound27, stage2 55, stage3 23,
presentation5. Parsed with `target/tutorial-presentation-receipts.py`:
8 distinct suites, **243 passed / 0 failed / 0 ignored**.
Existing HUD cache-conditional tests are not claimed as new font evidence;
the new synthetic presentation suite has no skip/early-return condition.
No full-workspace rerun, native app, server or GPU run was made.

`rustfmt --check --edition 2021 crates/client/tests/tutorial_presentation.rs crates/client/src/render/backend/cpu.rs`
and `git diff --check` pass. Pre-existing rustfmt debt in draw.rs at unrelated
cyclelogic sites is left alone. One ambiguous patch was rejected atomically
and retried after reading the file. Headless approval blocked execute_code
and a shell-inline filter; ordinary tools and a target-local receipt script
were used instead, without changing approvals/configuration.

## Handoff and remaining native proof

Implementation is ready for SAME-card profile `reviewer`; verified non-secret
profile defaults are grok-4.5 / xai-oauth, no task overrides. This is not review
acceptance. Root must obtain actual per-task approval, required corrective
whole-branch Grok4.6 review, fresh reviewed native build and bounded action/
logout proof. Offline pixel tests do not establish authentic assets, hardware
GPU presentation, live clicks/logout, cache/endpoint compatibility or
performance acceptance. Preserve the failed root `client-proof-arrow-0310`
action/logout session and its 300-second deadline result.

No input/packet/router changes, external checkout edits, live resources,
accounts/cache/endpoint changes, delegation, merge or push occurred.
Inherited untracked report and five tools were left untouched and unstaged.
