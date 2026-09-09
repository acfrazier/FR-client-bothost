# Composed ground-input diagnosis (offline, no production fix)

Task t_f8eda60b; branch `codex/revision-289-client`.
Examined HEAD `171cc31ab62f517cb9d51a9c77e5d012839618a1`, production
`03d916afe29808d76709f4ff91152059da6dfcd2`, baseline
`4f2048ea10f75b3bb92ff45610b35ba7313b0308`.
Only this report and `crates/client/tests/ground_input_composed.rs` belong to
this card. Existing untracked report/tools were left alone. STATE's top section
still points at the earlier presentation review; the task/parent handoff supplies
the newer accepted freeze. STATE is deliberately not edited under this card's
explicit two-file ownership restriction.

## Result and confidence

**No native root cause established.** The actual composed production input,
render-time terrain pick, and next simulation-loop packet path works in the
cache-free fixture on both CPU and an actual GPU adapter. Ordinary left-click
and right-menu Walk here selection both produce the expected R289 bytes. This
rules out an unconditional break in that composed path, not a scene-, pointer-,
collision-, timing-, or server-dependent failure.

An enclosed-player control concretely demonstrates another important boundary:
terrain picking and `tryMove` can succeed and emit MOVE_GAMECLICK to the player's
**existing source tile**, with `try_move_nearest == 1`. That is primary behavior,
not an introduced defect or a diagnosis of root's tutorial room. Consequently,
“player tile did not change” alone does not establish that input or packets failed.

High confidence: tested offline path, exact appended packet bytes, CPU/GPU
agreement, source-tile fallback. Low/unassigned confidence: which boundary failed
in root's native session. No corrective production behavior is justified yet.

## Preserved native evidence

Read-only root manifest:
`/Users/acfrazier/experiments/lostcity-289/runtime/client-proof-presentation-1109/proof-manifest.json`.
It names reviewed native SHA256
`b3b68d2e68d878fbee264c1f8a6fa7c39f3c8a9f880121a401343e2ed0266878`, wgpu,
room/player/NPC rendering, successful ordinary right-click menu, one logout-button
click returning to title, and native close exit 0 before the deadline.
Ground left-click and selecting Walk here left tile (3094,3106) unchanged.
NPC Talk-to was NOT tested. Captures were inspected by root in conversation;
there are no archived local screenshot files to remeasure here. The earlier
arrow-0310 action/logout failure remains rejected evidence, not overwritten by
this newer logout success. No live acceptance follows from this diagnostic.

Root's screenshot coordinate (80,150), pointer event coordinates, window origin,
and screenshot/window scaling remain a possible confounder. This task did not
replay native input, inspect a live window, or blame scaling. The manifest does
not contain a pick, route, packet, or server handler receipt for the failed click.

## Exact production seam

1. `client/present.rs:323-342`: winit CursorMoved writes its position directly
   into the cursor and shell; MouseInput forwards that cursor to mouse-down.
   `client/game_shell.rs:105-130`: latch transfers pending button/coordinates
   and clears only the pending button. In R289, mouse-down does not substitute
   for a pointer move. The fixture explicitly supplies both events.
2. `client/client.rs:11544-11569`: poll, one or more latch/mainloop passes,
   then renderer.mainredraw and presentation. `game_loop:11033-11130` handles
   inbound/telemetry/scene and pre-menu handlers, consumes the prior ground
   answer, then mouse_loop/minimap_loop. `build_minimenu:5929` belongs to drawing,
   not mouse-down. `render/draw.rs:492` other_overlays participates in that
   render-time menu flow. The fixture warms a real game_draw before input,
   calls full game_loop (not copied handler calls), draws, and ticks again.
3. `mouse_loop:9599-9721` either dispatches the last entry or opens a menu.
   A menu selection calls doAction while `is_menu_open` remains true, then
   closes the menu. Therefore menu closure alone is not proof of a valid row
   hit, much less a terrain pick. `doAction:3423-3435` uses saved row parameters
   for open-menu WALK, otherwise latched click, subtracting viewport offset4.
4. `core/world.rs:1055` update_mouse_picking arms click and resets answers.
   `Renderer::game_draw` runs begin/scene/composite/chrome/finish
   (`render/renderer.rs:351-368`). CPU scene calls render_all after camera and
   clipping setup (`render/backend/cpu.rs:133-240`). GPU scene performs the
   same setup, prepare_scene and build_scene_mesh (`backend/gpu.rs:1197-1328`).
5. `render/world.rs:2415-2455` builds marked GPU tiles and clears click on a
   successful pick. Quick-ground projection/depth/winding/inside-triangle
   checks are at 6148-6239 (second triangle follows); shaped ground's
   corresponding write is at 6057-6061. CPU quick-ground/shaped-ground writes
   occur at 4315, 4419, 4589. These are software hit tests during production
   geometry traversal even on GPU, not GPU framebuffer object-ID readback.
6. `game_loop:11110-11126` consumes ground_x, obtains the local route source,
   calls tryMove with nearest enabled, and sets crosshair mode1 on success.
   `tryMove:3602-3919` BFS reads current-plane collision, restricts to the
   104-square build area, considers a one-tile nearest ring, then emits run,
   absolute X/Z (local + map_build_base), and relative turn points. It does
   not move the local player simply because a request was emitted.
   `game_loop:11170-11185` writes the output buffer only if a stream exists;
   this fixture intentionally has none. It verifies emitted plaintext buffer
   bytes, NOT encrypted socket delivery, server acceptance, or a player update.

## Source comparison and server boundary

Primary read-only pin from plan.md: RuneWiki/openrs2-nonfree
`0c00ef249546fada67b1f6eb8bbe01ea7c250c95`, under
`/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/`.
No private primary source or real cache data was copied into the test.

- client.java 11013-11018: WALK718, saved menu coordinates versus latched click,
  subtract4, arm Class37.method564. Class37.java 1205-1214: arm and clear answers;
  1921-1952: front-facing projected triangle hit sets terrain X/Z.
- client.java 6030-6048: consume prior terrain answer through method206, reset
  sentinel, then pending-message acknowledgement and menu/minimap processing.
  Rust acknowledges before ground consumption, but neither acknowledgement nor
  this ordering changes the prior terrain answer; the tested no-message sequence
  does not depend on that ordering difference.
- client.java 10108-10125: nearest fallback accepts reachable tiles around the
  target, including the source. 10132-10181: retain a route point even at the
  source; MOVE_GAMECLICK234, variable length, run byte, p2 X then p2 Z.
- Base-to-HEAD diff is empty for core/world.rs, render/world.rs and
  dash3d/collision_map.rs. The client/backend diff was also inspected for the
  input/action/route seam: retained ground arm/consume and route algorithm,
  revision-selected packet IDs and previously reviewed input/presentation
  additions. This is not a baseline executable replay and does not classify the
  native symptom as a newly introduced regression. The source-tile fallback
  and render-deferred terrain mechanism are inherited, primary-backed behavior.

Server sources read only at engine HEAD
`a275ea812f34342cbaf7faf2e3558dcd6ca187d0`, content HEAD
`92649430fcbc83538d8c4367ecb96cee1a67a944`. Inspected files had no working diff.
Under `/Users/acfrazier/experiments/lostcity-289/engine/src/`:

- ClientGameProt.ts:96 declares MOVE_GAMECLICK234/-1.
- network/game/client/codec/MoveClickDecoder.ts:11-28 reads ctrl g1, startX/Z g2,
  then signed delta pairs (minimap alone reserves14 bytes). The tested payload
  `[0,0,50,0,51]` means run0 and one destination (50,51).
- handler/MoveClickHandler.ts:12-24 rejects `player.delayed` first, writing
  UnsetMapFlag. It also rejects invalid ctrl or start farther than104 tiles.
  Otherwise it clears pending action for game-click, sets tempRun, and either
  queues client waypoints plus walktrigger or server-findPath's destination
  route, depending on clientRoutefinder (30-54). Runtime routefinder setting,
  delay, and handler reception were not observed here.
- engine/script/handlers/PlayerOps.ts:359-379 sets bounded delayedUntil for
  arrival-delay and p_delay. World.ts:692 clears expired delay, then can resume
  suspended scripts. Delay is a real rejection condition, not proof that root's
  player was delayed when clicked.
- content/scripts/tutorial/scripts/tutorial.rs2:30-55 initializes tabs/hints,
  opens design for the initial step; tutorialstep.rs2:1-45 sends tutorial text.
  These inspected opening/text routines do not establish a permanent walking
  prohibition. Other tutorial skills/doors contain delays. No account/tutorial
  stage state was read, and no blanket “tutorial blocks walking” claim is made.

Initial scene bounds/collision: the fixture uses the real 104-square collision
map with default open interior and blocked boundaries (collision_map.rs:45-53).
The enclosed variant adds production block_ground around (50,50), proving a
valid but non-displacing fallback. Root's absolute position alone does not give
its current build base, local source, clicked tile, plane, collision flags, or
reachable route. Those native values cannot be reconstructed from this manifest
without an additional observation; no real cache/account dependence was added.

## Synthetic proof and its limits

The new integration file reuses the projection recipe from tests/world.rs and
GpuBackend::try_new / FrameOutput texture readback infrastructure. It constructs
its own absent cache path, asserts absence, seeds plain terrain in a full-sized
World, and renders through actual Renderer/game_draw. No helper assigns menu
entries, world.click, or ground_x/z. The output buffer is not reset to discard
telemetry; only the exact new movement suffix after the draw is asserted.

Each backend exercises left-click and real menu row selection in:

- translated height2000/pitch512 flat-world reference geometry;
- pitch256 (legal game pitch), eye height1000 and Z offset1000, using initialized
  visibility backing rather than the height2000 visibility bypass;
- that oblique geometry with enclosed-player collision.

Nonzero pixels are required at the independently projected terrain point, in
CPU scene pixels or actual GPU texture readback. For both unblocked geometries,
render picks (50,51) and the next loop appends exactly
`[234,5,0,0,50,0,51]`. Enclosed-player variants pick the same tile but append
`[234,5,0,0,50,0,50]`, nearest1. Every case checks sentinel consumption,
crosshair1, no inline render-time movement emission, and no duplicate move on a
subsequent frame/tick without mouse-down. GPU is ignored by default and requires
an adapter with expect; its explicit run cannot silently skip on missing GPU.

Limits: synthetic fixed camera, plain terrain, no authentic room/walls/bridges,
no real NPC or tutorial UI models, no loaded map-build collision, base coordinates
zero, no ISAAC/socket/server, and no native event-loop timing/coordinate mapping.
The oblique view is not an orbit-camera or tutorial-room replay. GPU execution
proves the real backend path here, not the failed native screenshot's geometry.

## Commands and preserved receipts

All commands ran serially here, with
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target` and
`--all-features`. Logs are local evidence under that target directory.

- Initial `cargo test -p client --all-features --test ground_input_composed -- --test-threads=1 --nocapture`:
  `ground-input-initial.log`, exit101: E0616 accessing private World.groundh.
  Corrected fixture to the existing public World::new constructor; no production
  visibility change. This was a fixture compile failure, not a behavior RED.
- Same command: `ground-input-cpu-first.log`, exit0, CPU1 passed/GPU1 ignored.
- `cargo test -p client --all-features --test ground_input_composed gpu_ground_input_composed -- --ignored --exact --test-threads=1 --nocapture`:
  `ground-input-gpu-first.log`, exit0, 1 passed/0 ignored.
- Expanded geometry/collision runs preserve `ground-input-cpu-expanded.log`
  (CPU1 passed/GPU1 ignored) and `ground-input-gpu-expanded.log` (1 passed,
  0 ignored), both exit0.
- Final `cargo test -p client --all-features --test ground_input_composed --test walk --test input --test minimenu -- --test-threads=1 --nocapture`:
  `ground-input-final-affected.log`, exit0: composed CPU1 passed/GPU1 ignored;
  input16, minimenu27, walk6 passed; no failed tests.
- Final explicit GPU command above: `ground-input-final-gpu.log`, exit0,
  1 passed/0 failed/0 ignored, with all geometry/collision subcases logged.
- `rustfmt --edition 2021 --check crates/client/tests/ground_input_composed.rs`
  and `git diff --check`: exit0. No full-workspace run.

An exploratory execute_code call was denied by headless approval policy; ordinary
tools were used instead. No policy/configuration workaround was attempted.
No live client, window/CUA, server, socket login, account, cache mutation,
external checkout edit, delegation, production fix, merge, or push occurred.

## Minimal next observation for root (not implemented here)

One root-owned, opt-in, bounded **single-click boundary trace** is the next proof,
not another blind action replay or behavior patch. Correlate one intended ground
click across: physical window/cursor and latched coordinates; selected menu action
and pending-message status; scene state/camera/armed pick and post-render ground
answer; local source/build base/plane, tryMove result/nearest/path endpoint and
movement plaintext fields; stream write result. Exclude credentials, ISAAC state,
chat text, and unrelated packets. This can classify the missing boundary before
adding broad diagnostics or changing behavior. If valid remote displacement
coordinates demonstrably leave the client, the same bounded observation needs
root's server receive/decoder, delayed and handler-return/queued-endpoint evidence;
client bytes alone still cannot diagnose server rejection or player updates.

Falsifiable alternatives, ordered by boundary rather than guessed blame:
1. No WALK/armed pick: pointer/menu/pending-message boundary (menu closure is
   insufficient evidence). 2. Armed but no terrain answer: native scene/pick
   boundary. 3. Answer but no move or source-tile move: collision/route boundary.
4. Displacing move written but no displacement: transport/server gate/path/update
   boundary. This fixture passes the first three with open synthetic geometry and
   separately establishes the source-fallback counterexample.

Same-card profile reviewer is required next; verified configured default is
`grok-4.5` / `xai-oauth`, with no task override. Root owns any later correction,
required review, native proof, and campaign acceptance. This report is not a
request to change production behavior and is not whole-client acceptance.
