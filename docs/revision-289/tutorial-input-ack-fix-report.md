# Tutorial input acknowledgement correction

Task: t_08298357. Implementation boundary: 2026-09-09 03:28 UTC.
Prepared branch verified: `codex/revision-289-client`.
Starting HEAD: `411ef132193f7b65d58f6b0ec55b6d817a32985e`.
Legacy comparison: `4f2048ea10f75b3bb92ff45610b35ba7313b0308`.

## Defect and minimal correction

`handle_chat_if_clicks` ran before `mouse_loop` and `minimap_loop` but after
tabs. It zeroed every nonzero click whenever `tut_com_id != -1`, even with no
message pending. This explains root's tab-success/action-failure observation;
it is not an offline claim that the live session is repaired.

The primary condition is `super.anInt191 == 1 && this.aString4 != null`
(Java 6042-6046), not interface presence, nonempty text, or any mouse button.
The correction applies that condition only to R289: a left click clears the
pending message, marks chat dirty, and clears the click, without closing the
tutorial interface or emitting a packet. Other clicks continue to the existing
menu/action machinery. No action/opcode, telemetry, timer, protocol or gameplay
policy was changed.

`Option<String>` now represents the Java state explicitly: `None` means no
acknowledgement; `Some("")` is a valid empty pending message. Construction and
cold login use `None`; a kind-0 notice with an active tutorial uses `Some(text)`.
The required draw consumer adapts to the option instead of testing emptiness,
so an empty pending message still selects its existing acknowledgement hint.
No NPC overlay, GPU signature, freeze, or renderer ownership change was made.

## Primary audit and lifecycle decisions

Read-only primary: `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/client.java`.
All `aString4` occurrences were inspected, not just the input condition:

- 160-161: nullable field, initially Java null.
- 1819-1824, `method99`: kind 0 plus active `anInt271` assigns the supplied
  string (including empty) and clears the current click. This is the shared
  chat insertion path, not only MESSAGE_GAME. The prior Rust capture was
  MESSAGE_GAME-only. Capture is now in revision-gated `add_chat`, fixing local
  kind-0 notices as well without affecting other chat kinds or R274.
- MESSAGE_GAME calls that shared method at 3279; other local kind-0 calls
  include friend/ignore notices (5776-5787, 8307-8320) and action notices
  (11071, 11087, 11203, 11253, 11409, 11571, 12305).
- 5273-5275: pending message is selected by non-nullness, with the continue
  hint even if its text is empty.
- 6042-6046: LEFT plus non-null clears to null, redraws chat, consumes click.
- 8472-8482: cold response 2 clears tutorial/interface state and the message.
- 9832-9834: pending message marks chat dirty during Java drawing.

There are no other assignments to `aString4`. In particular IF changes,
TUT_OPEN, logout, and response 15 do not clear it in primary. TUT_OPEN at
2992-2997 changes the interface and dirty flag only. Rust retains the pending
message across these events and clears it on cold login, not on IF_CLOSE or
TUT_OPEN(-1). Existing Rust logout interface teardown is unchanged. Reconnect
15 retains both message (including empty) and tutorial ID; cold 2 clears both.
The input condition remains independent of interface presence after a close.

### Deliberately unmodified presentation boundary

The draw audit found a pre-existing difference: Rust currently draws pending
text inside its tutorial-IF branch, after the chat-modal branch; Java selects
the pending-message branch before chat/tutorial interfaces. Java also requests
a redraw each frame while pending. This task forbids renderer changes, so the
only draw edit is the necessary nullable/empty consumer adaptation. It does
not correct precedence or add a per-frame redraw policy. Consequently these
tests establish input/state behavior, not full Java tutorial presentation
parity, especially if an IF is changed while a message remains pending. This
finding is recorded on the card for root's separate scope decision.

## Deliberate 274 preservation

`git grep` of base `4f2048` shows `handle_chat_if_clicks` was a no-op at line
5036, invoked before menu dispatch. The base has no `tut_com_message` field
or tutorial-message draw consumer. Both new capture and acknowledgement are
R289-gated, preserving 274's prior no-op input semantics rather than retaining
the cleanup-introduced swallowing bug. A 274 tutorial with server and local
kind-0 notices leaves message state absent; left click emits the independently
fixed legacy IF_BUTTON `[9, 0, 2]`, and right click opens its menu.

## Regression proof and fixture provenance

Expanded `revision_289_stage2.rs`'s cleanup_f coverage without weakening its
original challenge-body or nonempty-message checks. Added synthetic interface
and NPC configuration, not live cache/account/server fixtures. Tests invoke
actual shell mouse-down/latching, render-time `build_minimenu`, the ordered
pre-menu handlers from `game_loop`, `mouse_loop`, and `minimap_loop`.
They do not assign an expected post-handler click or call only `doAction`.
This is a bounded input subsequence, not a full native frame/render proof.

- Active tutorial, no message: widget dispatch sends exactly `[86, 0, 2]`.
- Nonempty and empty pending messages: arrival consumes an old click; the
  next LEFT acknowledges locally once; the following LEFT dispatches widget.
- RIGHT with no/nonempty/empty pending message opens a real menu and preserves
  message state. Selecting the menu with LEFT acknowledges first when needed;
  a subsequent LEFT dispatches exactly one widget packet.
- Synthetic NPC pick builds the real NPC menu entry, then dispatches after
  acknowledgement or immediately without a message. Expected output is
  `[67, 5, 0, 0, 5, 0, 5, 252, 0, 7]`: source-tile MOVE_OPCLICK followed by
  OPNPC1 slot 7. Crosshair state becomes 2.
- Subsequent ticks with no new mouse-down never add a duplicate packet.
- Replacement with empty text, local kind-0 capture, TUT_OPEN/IF_CLOSE state,
  acknowledgement after tutorial close, cold login, reconnect and logout are
  covered through production methods/packet paths.

Independent output references: Java 11234-11242 (IF_BUTTON 86, p2 component);
11208-11231 (OPNPC1 252, p2 slot); 10132-10181 (one route point is retained
at the source tile, MOVE_OPCLICK 67, length 5, run byte then p2 X/Z).
No expectation is generated with the production packet-emission helper.

## Exact commands, failures, and final results

All tests ran serially in this checkout with
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target` and
`--all-features`; no concurrent Cargo job or live proof was started here.
Logs below are retained under that target directory.

1. `cargo test -p client --all-features --test revision_289_stage2 cleanup_f_tutorial_without_message_dispatches_widget_once -- --exact --test-threads=1`
   - `tutorial-input-red.log`: exit 101, initial test import error E0432;
     corrected by using the existing `config::if_type` import path.
   - `tutorial-input-red-behavior.log`: exit 101, actual regression RED:
     emitted `[]`, expected `[86, 0, 2]` (0 passed / 1 failed).
2. `cargo test -p client --all-features --test revision_289_stage2 cleanup_f -- --test-threads=1`
   - `tutorial-input-green-initial.log`: exit 0, 7 passed after nullable input fix.
3. `cargo test -p client --all-features --test revision_289_stage2 cleanup_f_tutorial_local_kind0_uses_same_pending_state -- --exact --test-threads=1`
   - `tutorial-input-red-local.log`: exit 101, 0 passed / 1 failed:
     local kind-0 left `None`, expected `Some("")`.
   - Moving capture to revision-gated `add_chat` makes the cleanup_f command
     above pass 8 tests: `tutorial-input-green-local.log`, exit 0.
4. `cargo test -p client --all-features --test revision_289_stage2 -- --test-threads=1`
   - `tutorial-input-stage2.log`: exit 101, 54 passed / 1 failed. The new NPC
     test initially expected no movement packet for a co-located target.
     Primary 10132-10181 instead retains a source-tile point; corrected only
     the test's independently specified byte oracle to include that packet.
     Production movement code was not changed; the failed receipt is retained.
5. `cargo test -p client --all-features --test revision_289_stage2 --test revision_289_outbound --test input --test hud --test minimenu --test walk -- --test-threads=1`
   - `tutorial-input-regression.log`: exit 0. stage2 55, outbound 27, input 16,
     hud 86, minimenu 27, walk 6 passed; no failures or ignored tests.
6. `cargo test -p client --all-features --test revision_289_stage3 --test do_action --test social --test chat_mode --test game_shell --test login --test logout --test lost_con -- --test-threads=1`
   - `tutorial-input-legacy.log`: exit 0. stage3 23, do_action 13, social 23,
     chat_mode 4, game_shell 3, login 10, logout 7, lost_con 4 passed;
     no failures or ignored tests.

Final two receipts were parsed and checked: 14 distinct suites, **304 passed,
0 failed, 0 ignored**. `git diff --check` passes. The patch tool reports
pre-existing rustfmt differences elsewhere in the touched files; new test
formatting was corrected without a broad formatting pass. No full-workspace
rerun was needed. The explicit ignored GPU-only proof convention is untouched;
this task does not claim a new GPU proof.

Two exploratory tool attempts were blocked by headless approval policy
(`execute_code` and a `python -c` shell filter); normal read/search/git tools
were used instead. No configuration was changed to bypass approvals.

## Review and live boundary

Implementation is ready for same-card profile `reviewer` (configured
`grok-4.5` / `xai-oauth`, no task model override). Root must obtain actual
review acceptance and required corrective Grok4.6 review before a fresh build
and bounded native action/logout proof. This implementation is not review
acceptance and does not satisfy live logout or performance acceptance.

The upstream NPC overlay review `t_e7dfd2e6` accepted the arrow correction
bounded offline; root reports live blink on/off and arrows following moved
NPCs. Root's prior `client-proof-arrow-0310` session still failed game actions
and logout and ended at its declared 300-second deadline. Preserve that
failed action/logout evidence. No live server, CUA, account, external checkout,
cache, endpoint, or host integration was touched in this task; no push/merge.

hotspot: `crates/client/src/client/client.rs` — serialized campaign file;
only message state/capture/acknowledgement changes belong to this correction.
