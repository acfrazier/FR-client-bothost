# Bounded ground-input boundary trace

Task `t_2ebebe02`, based on accepted diagnostic `096864e367ea765c950d54f674f2321f42cff15b`,
branch `codex/revision-289-client`. Diagnostic instrumentation only: no native
root cause, displacement, server acceptance, or whole-client acceptance claimed.
The earlier native ground failure and the separate successful menu/logout proof
remain as recorded in STATE and ground-input-diagnostic-report.md.

## Opt-in and bound

Set exactly `CLIENT_289_GROUND_TRACE=1` in the actual client process environment.
Unset, `0`, and `true` are disabled. This is separate from `BOT_DEBUG`. Revision
274 does not even read this environment variable. Revision289 reads it once in
Client construction; disabled hot paths do not format, allocate trace buffers,
read window sizing, or read the clock for this diagnostic. The state is a fixed
size Option in the existing GameShell owner, not a global collector or router.
The winit owner retains another Arc reference to its existing window (no new
window/surface allocation) to obtain sizing only on opted-in mouse-down.

For root's reviewed native launch, add `env CLIENT_289_GROUND_TRACE=1` before the
existing executable invocation and retain `--revision 289` and the existing
root-owned host/port/cache arguments unchanged. Do not launch the pre-review
binary on this card. The exact standalone synthetic invocation is:

    CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target CLIENT_289_GROUND_TRACE=1 cargo test -p client --all-features --test ground_input_composed cpu_ground_input_composed -- --exact --test-threads=1 --nocapture

One observation per constructed client, with a process-local numeric `id` and
increasing `seq` on every line. Native root should correlate within one process,
not concatenate IDs across separate runs. The first in-game input starts the
attempt; a right-click followed by its left menu selection shares that ID. A
further click after arming terminates as `superseded_input`, rather than attaching
its later movement to the previous click. Coalesced multiple downs terminate as
ambiguous instead of claiming a unique click. Menu closure without WALK is not
success, including hover closure before selection. No re-arm command exists.

Maximum 64 lines per owner, including one explicit `stage=complete`. The waiting
lifetime is 120 seconds from first in-game game_loop; the attempt lifetime is
10 seconds from its first observed input. Expiry is checked on simulation passes
and before emission, so a late render cannot log a successful stage past its
deadline. This is cooperative, not a watchdog thread: a blocked/stopped event
loop cannot emit its timeout until execution resumes. Drop/logout/lost-connection/
session-transfer close the observation explicitly. Abrupt process kill or a
broken stderr sink can prevent a completion receipt; absence is never success.
No per-frame trace lines, event Vec, string accumulation, packets, or replay
input are introduced. Stderr writes are best effort; use a regular file for
root's receipt rather than an undrained pipe.

## Field meanings and boundaries

Every production trace line starts `ground289`. Apart from fixed labels/reasons,
fields are numeric. No raw packets, ISAAC state, error messages, credentials,
account names, chat text, tutorial message text, or unrelated payload are read
by the diagnostic. Tests deliberately keep their earlier synthetic byte oracle;
that fixture's pre-existing byte print is not production trace output.

- `input`: actual apply_mouse_down button/x/y snapshot, click ordinal, number of
  downs coalesced since the preceding observed pass. `latch`: button/x/y that
  game_loop actually sees after the ordinary shell latch. A recorded down with
  no latched button terminates `no_latched_input`. Title-screen downs are ignored
  on the first in-game pass with no latched button. `downs=0` distinguishes callers
  which set shell fields without apply_mouse_down (not native winit input).
- `native`: cursor physical pixel coordinates after the existing winit f64-to-i32
  conversion, physical inner width/height, scale factor multiplied by 1,000,000,
  logical inner width/height multiplied by 1,000 (rounded). Sampled at the matching
  left/right MouseInput press, before apply_mouse_down. The applet remains the
  existing 765x503 physical surface, with its 512x334 viewport at offset (4,4).
  No scaling conversion is added to input. `available=0` means host/synthetic
  input, not a fabricated native event. Fractional cursor values before the
  existing integer conversion and global window/screenshot origin are not logged.
  Root must still retain the screenshot's coordinate-space context.
- `input_state`: pending-message presence/menu-open/scene at game_loop entry.
  `dispatch_state`: presence/open/remaining button after inbound processing and
  obj-drag, before the ordinary click handlers. Only presence, including an empty
  Some message, is logged. `selection`: whether the actual doAction selection is
  WALK and whether it used the open menu. No unrelated action parameters/text.
- `walk_arm`: actual world click coordinates after the original viewport offset,
  scene state, and saved-menu versus direct-click branch. `menu_after` reports
  open/armed/remaining button without treating closure as action dispatch.
- `scene`: camera X/Y/Z, pitch/yaw, scene state and click flag before backend.scene,
  after backend.begin. This is the pre-scene camera, not a new camera calculation
  or an assertion that shake/clipping inside a backend did not occur.
- `post_render`: the first backend.scene result after arm, including ground X/Z,
  click flag and scene state. X=-1 ends `no_pick_first_render`. This reports that
  first observation only; it does not prevent a later frame from picking, and
  does not classify a frozen scene1 as a terrain failure. No CPU/GPU ownership,
  scene1 last-FBO policy, or picking code is changed.
- `source`: local-player presence, local route source X/Z, map build base X/Z,
  collision plane, and consumed ground target. `route`: tryMove return, nearest
  flag, and full-route minimap endpoint in local tiles (or -1 on failure).
- `movement`: only while the traced ground-consume call is inside tryMove;
  type0, revision-mapped plaintext opcode, payload length, run0/1, absolute
  first waypoint X/Z (the existing u16 wire representation), and transmitted
  waypoint count (at most25). `movement_delta` gives each signed wire delta
  relative to that first waypoint, in emission order. The full route endpoint
  can differ from the last transmitted waypoint for a truncated route; reconstruct
  the latter from these fields. This is recorded at the unchanged emission seam,
  not by decoding or logging the output buffer.
- `write`: emitted only when that traced ground call emitted a movement request,
  immediately after the ordinary same-loop stream.write call. Stream presence;
  result1 = returned Ok, -1 = returned Err, 0 = no write. `stream_write_ok` is
  strictly the ClientStream API return: TCP uses a writer queue, and a closed
  stream's write can be a no-op. It is NOT proof that bytes left the process,
  reached the server, passed a delay gate, or moved the player. No async writer
  instrumentation or real socket success/error proof is added on this card.
- `complete`: explicit reason. `seen` is a stage-presence bitmask: arm1,
  post-render2, source4, movement8, route16, write32. In particular `seen=63`
  is NOT a success mask: the no-stream fixture also has all these observations.
  Other terminal reasons include `no_walk_selected`, `no_local_source`,
  `route_failed`, `coalesced_input`, the two timeouts, `event_cap`, and owner/
  session termination. Missing stage bits remain missing, never inferred.

## Verification and retained failures

All tests/build checks below ran serially in this checkout using
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`. No full
workspace run, native application, CUA, login, live server/account/cache changes,
external checkout edits, delegation, integration, merge or push occurred.

Preserved local receipts under target/:

- `ground-trace-red.log`: actual RED, exit101; the new subprocess receipt test
  failed on missing input trace before production instrumentation existed.
- `ground-trace-green-first.log`: same exact `ground_trace_receipt` test passed.
- `ground-trace-expanded.log`: CPU composition, default/invalid flags, R274,
  pending-message privacy and actual renderer no-pick controls passed.
- `ground-trace-expiry-red.log`: actual RED, exit101; a render event past the
  attempt deadline still emitted. The event-time expiry check corrected it.
- `ground-trace-latch-red.log`: actual RED, exit101; an observed down without
  latch yielded only owner_drop. Bounded down accounting now distinguishes it.
- Initial unit and actual-GPU receipts remain `ground-trace-unit.log`,
  `ground-trace-unit-final.log`, and `ground-trace-gpu.log`.
- A combined Python `-c` aggregation/check command was blocked by headless
  approval policy before execution. No approval/config workaround was used;
  ordinary search_files/read_file and standalone Cargo/rustfmt commands supplied
  the evidence instead. Ambiguous V4A hunks also failed atomically before edits.

Final exact commands and results:

    cargo test -p client --all-features --test ground_input_composed --test walk --test input --test minimenu --test do_action --test game_shell --test present_pack --test render_backend --test revision_289_outbound -- --test-threads=1 --nocapture

`ground-trace-final-affected.log`, exit0: do_action13, game_shell3,
ground_input_composed6 passed/1 ignored, input16, minimenu27, present_pack2,
render_backend5, revision_289_outbound27, walk6; no failed tests. The composed
receipt runs the production CPU sequence in subprocesses with absent/0/true/1
flags, requires exact existing movement bytes, one arm/emission/completion per
fixture, and source-fallback fields. The R274 subprocess exercises full
input/draw/consume and requires no diagnostic output. No-pick, pending-message,
and missing-latch controls require their distinct terminal reasons and no
movement trace. A message sentinel must not appear in output.

    cargo test -p client --all-features --lib ground_trace_289 -- --test-threads=1 --nocapture

`ground-trace-final-unit.log`, exit0: 4 passed. Fixed-cap/terminal idempotence,
waiting/attempt deadlines, event-time expiry, right/menu correlation, no relabel
on a further armed click, and the R274 opt-in gate.

    CLIENT_289_GROUND_TRACE=1 cargo test -p client --all-features --test ground_input_composed gpu_ground_input_composed -- --ignored --exact --test-threads=1 --nocapture

`ground-trace-final-gpu.log`, exit0: 1 passed/0 ignored. Actual adapter required
with expect; actual texture readback required. Both direct and menu input in
reference/oblique/enclosed geometry retain exact movement bytes. Each fixture
has one correlated trace through pick/route/movement/no-stream completion.
The enclosed case reports nearest1 and absolute destination (50,50), matching
source-tile fallback; unblocked cases report (50,51). No adapter skip is allowed.

    cargo check -p client --no-default-features
    cargo check -p client --all-features

`ground-trace-default-check.log` and `ground-trace-window-check.log`: exit0.
The latter also compiles the final left/right-only native metadata sampling
condition. Native winit coordinates/scaling themselves were not exercised.

Scoped rustfmt check passed for ground_trace_289.rs, game_shell.rs, present.rs,
render/renderer.rs and tests/ground_input_composed.rs, with `--edition 2021
--config skip_children=true --check`. `git diff --check` passed. The existing
compact style in client.rs/mod.rs and unrelated formatting debt were retained;
no whole-file rustfmt/clean-workspace claim is made.

## Handoff

Same card must be reviewed by configured `reviewer` (verified grok-4.5/xai-oauth,
no card override). Root obtains the required corrective whole-branch Grok4.6
pass before a fresh native build/trace. This instrumentation does not justify
any input, camera, collision, packet or lifecycle fix. Root still owns the native
observation and any later transport/server receive/decoder/delay/queue evidence.

hotspot: crates/client/src/client/client.rs — serialized campaign owner touched
only for diagnostic initialization/session termination, selected action, ground
consume/emission and flush observations.
