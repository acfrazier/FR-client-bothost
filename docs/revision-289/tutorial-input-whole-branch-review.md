# Revision 289 tutorial input: required Grok 4.6 whole-branch review

Task: `t_7fe3337b`
Role: required final `branchreviewer` (not a section review, not live/release authorization)
Model: grok-4.6
Provider: xai-oauth (profile defaults; no task model/provider override)
Worker session: `20260909_064019_cd1ea8`
Kanban run: 954
Date (local): 2026-09-09 EDT

## Verdict: OFFLINE ACCEPTED (bounded)

Independent Grok 4.6 whole-branch review of `codex/revision-289-client` after
the tutorial-input acknowledgement correction. This is **not** whole-client
acceptance, **not** live action/logout proof, **not** authentic cache pairing,
and **not** performance acceptance. It is **not** Java tutorial-presentation
parity.

Predecessor receipts are **preserved**, not rewritten:

- `t_77d35bfb` REJECTED `0030afb` — `docs/revision-289/branch-review.md`
- `t_615aa9ac` OFFLINE ACCEPTED (bounded) `0e3b6a7` — `docs/revision-289/branch-rereview.md`
- `t_f4e2ad64` OFFLINE ACCEPTED (bounded) `fc5516c` — `docs/revision-289/branch-final-review.md` (still untracked in this tree; identity kept)
- Cleanup A–H code `0406ceb` aggregate Grok 4.5 `e4834d7` and whole-branch Grok 4.6 `0227f3f` (`t_d1a06f94`)
- NPC overlay `1b38f18` + GPU-test portability `b03e633` + report `411ef13` (`t_e7dfd2e6` actual `20260908_230411_5b9c2c`)
- Parent same-card `t_08298357` APPROVED `91fcadd` (actual Grok 4.5 / xai `20260908_233218_3ac352`)

Those documents are evidence. They are not a waiver to skip code.

## Frozen identity

Frozen **before** this review read production or tests:

| Field | Value |
| --- | --- |
| Branch | `codex/revision-289-client` (verified) |
| Original R274 published base | `4f2048ea10f75b3bb92ff45610b35ba7313b0308` |
| Prior whole-branch report (NPC) | `411ef132193f7b65d58f6b0ec55b6d817a32985e` |
| Combined overlay production | `1b38f1854d4c7b59e9c9c00b335df0255bc578c4` |
| GPU-test portability | `b03e633ffdcbb4c2903b6b569497aa3d4b7b9a00` |
| Parent-reviewed production HEAD | `91fcadde87d9566f7844de4d18b8823fa8dd09fe` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| `CARGO_TARGET_DIR` | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by this reviewer | none |
| Follow-up cards created | none |

Code freeze check (this run):

- `git rev-parse HEAD` equals `91fcadde87d9566f7844de4d18b8823fa8dd09fe`.
- `git status --porcelain` has **no tracked modifications**. Inherited untracked non-deliverables left unstaged: `docs/revision-289/branch-final-review.md` and five `tools/*` tracers.
- Parent Grok 4.5 reviewed this same full hash. No source change during the usage pause.
- `git diff --stat 411ef13..HEAD` is the five-file input correction only: `client.rs`, `draw.rs`, `revision_289_stage2.rs`, `STATE.md`, `tutorial-input-ack-fix-report.md`.
- Overlay GPU/renderer files are unchanged after `1b38f18` except the already-reviewed ignore-metadata in `gpu_overlay_tests.rs` (`b03e633`) and the Option consumer in `draw.rs` (`91fcadd`). `freeze_last_scene` remains `kind == Game && scene_state == 1`.

Span `4f2048e..HEAD`: 53 commits.

## Scope read

- Local `AGENTS.md`, `docs/revision-289/STATE.md` current top, `tutorial-input-ack-fix-report.md`.
- Primary 289 Java (read-only): `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/client.java` — all eight `aString4` sites, method99 1819-1824, TUT_OPEN 2992-2997, MESSAGE_GAME 3279, draw 5267-5382, LEFT ack 6042-6046, cold login 8472-8482, per-frame dirty 9832-9834, IF_BUTTON 11234-11242, OPNPC1 11208-11231, MOVE_OPCLICK 10132-10181.
- Production: `Client::tut_com_message`, `add_chat`, `handle_chat_if_clicks`, `game_loop` pre-menu order, cold login, logout, `draw_chat`.
- Base `4f2048e` `handle_chat_if_clicks` at line 5036 is a no-op. No `tut_com_message` field there.

Lens: artifact-first cold read of nullable state, capture, ack, draw, lifecycle, then comparison to the parent handoff, then independent serial execution. Full workspace 950-test suite was **not** repeated.

## Combined input correction (primary attention)

### Defect (source path, not packet inference)

Cleanup-era `handle_chat_if_clicks` zeroed every nonzero click whenever
`tut_com_id != -1`, even with no pending message. That runs after tabs and
before `mouse_loop`/`minimap_loop`, which matches root's tab-success /
action-failure observation. It is not a claim that the failed live session is
repaired.

Primary 6042-6046 is `super.anInt191 == 1 && this.aString4 != null` only:
clear to null, dirty chat, consume the click. No interface predicate, no
nonempty-text predicate, no packet, no tutorial close.

### Input / state findings (accepted)

- `Option<String>`: `None` is Java null (no ack); `Some("")` is a pending
  empty message. Construction and cold response 2 use `None`.
- Capture is revision-gated in `add_chat` for `type == 0 && tut_com_id != -1`,
  matching method99 1821-1823 (shared path, including local kind-0 and empty
  text). MESSAGE_GAME still reaches it through `apply_message_game`. R274 is
  not captured.
- Arrival still zeroes `mouse_click_button`, matching method99 `anInt191 = 0`.
- Ack is R289 + LEFT + `is_some()` only. Right click passes through. A second
  LEFT after ack reaches widget/NPC. Subsequent ticks with no new mouse-down
  do not duplicate output.
- TUT_OPEN / IF_CLOSE do not clear the message (Java has no other `aString4`
  assignments). Ack after `TUT_OPEN(-1)` still consumes one LEFT — Java 6042
  has no `anInt271` check. Logout keeps the message and still clears
  `tut_com_id` via existing TS-shaped teardown. Reconnect 15 keeps both;
  cold 2 clears both.
- 274: capture and ack are gated off; widget LEFT still emits legacy
  IF_BUTTON `[9, 0, 2]`.

Packet oracles are independently specified from Java, not from the emit
helper: IF_BUTTON 86 + p2 component; OPNPC1 252 + p2 slot 7; MOVE_OPCLICK 67
length 5 with a retained source-tile point. The preserved RED
`tutorial-input-stage2.log` expected `[252, 0, 7]` while production emitted
`[67, 5, 0, 0, 5, 0, 5, 252, 0, 7]`. The test oracle was corrected to Java
10132-10181; production movement was not changed to match a weaker
expectation.

`tutorial_input_pass` is a bounded pre-menu subsequence (obj drag, tabs,
side/main/chat IF, chat mode/input, `mouse_loop`, `minimap_loop`). It omits
the `game_loop` ground-walk block. Widget/NPC tests do not rely on
`world.ground_x`. This is not a full native frame proof.

Handler order vs Java remains the existing Rust/TS loop: tabs then ack then
walk/`mouse_loop`. Java 6030-6049 is walk then ack then method96/225/188.
`handle_tab_clicks` does not consume `mouse_click_button`, so a LEFT on a tab
while pending can both switch tab and ack. Pre-existing sequence; not
introduced by 91fcadd. Not treated as an input-condition defect.

## Material remaining presentation finding

Root asked this pass to classify draw.rs 3062 against primary method129
J5267-5382, including colour. Prior card-scope deferral is **not** Java
parity and does **not** waive a whole-branch finding.

Java chat-area order (5267-5382):

1. social prompt
2. enter-amount prompt
3. **`aString4 != null`**: centre message at colour **0**, then
   `"Click to continue"` at colour **128**, and **do not** draw chat modal or
   tutorial IF
4. else chat modal (`anInt408`)
5. else if `anInt271 == -1`: ordinary chat
6. else: tutorial IF only (`method119` at 5381) — reached only when the
   pending string is null

Rust `draw_chat` (3024-3077):

1. social prompt (second line already `Colour::DARKBLUE` = `0x80`)
2. enter-amount (second line `DARKBLUE`)
3. **chat modal wins**
4. else if `tut_com_id != -1`: draw tutorial IF, **then** overlay pending
   text and `"Click to continue"` both `Colour::BLACK` (`0`)
5. else ordinary chat — **pending `Some(_)` is not drawn at all**

`Colour::DARKBLUE` is `0x80` / 128. The hint colour is a concrete mismatch
with J5275. Social/amount already use the Java second-line colour; the
tutorial hint does not.

### Effect on visibility

- Happy path (tutorial IF open + pending): Rust still shows the message, but
  composites it over the IF. Java replaces the IF with the prompt. Empty
  `Some("")` still selects the hint in both (Java non-null; Rust `is_some()` /
  `if let Some`).
- Chat modal open + pending: Java shows the continue prompt; Rust shows the
  modal.
- Tutorial closed / `tut_com_id == -1` + leftover pending: Java still shows
  the continue prompt. Rust shows ordinary chat with **no hint**.

Java 9832-9834 also dirties chat every frame while pending. Rust dirties on
capture/ack only. That can stall a prompt that is otherwise in the tut-IF
branch until some other chat dirty. Secondary to the branch-order defect.

### Effect on ack interaction

Input 6042 does **not** consult draw. After 91fcadd, LEFT + `is_some()`
consumes the click even when the prompt is not on screen.

That coupling is new relative to the cleanup swallow (`tut_com_id != -1`)
and is the live confounder: after IF close or TUT_OPEN(-1) with a leftover
message, the next LEFT is a silent ack. Java would still show `"Click to
continue"`. Tests
`cleanup_f_tutorial_message_state_is_independent_of_interface_transitions`
assert the Java input outcome, not presentation. They are not a claim of
draw parity.

Right click is unaffected (button != 1). 274 is unaffected.

### Effect on final branch acceptance

This **does not reject** 91fcadd's input condition, Option model, kind-0
capture, 274 no-op, or NPC overlay freeze. Those match the cited Java input
and lifecycle sites and the independent serial tests.

This **does reject Java tutorial-presentation acceptance** for the combined
branch. It is a **material remaining finding**, not a nit.

Required correction (root-owned scoped draw card; this reviewer does not
self-fix):

1. Reorder `draw_chat` after the social/amount prompts to match J5267-5382:
   pending `tut_com_message.is_some()` (including empty) draws message + hint
   and skips chat-modal / tutorial-IF / ordinary chat.
2. Draw `"Click to continue"` with `Colour::DARKBLUE` (128); keep message
   text `Colour::BLACK` (0).
3. Optionally dirty chat every frame while pending (J9832-9834).

Until that lands, live click/logout proof cannot fairly distinguish ack-logic
failures from an invisible leftover pending message. Do **not** treat a
single stolen LEFT after tutorial close as evidence that 91fcadd is wrong.

## Tests actually run (this reviewer)

Serial, this checkout, `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`,
`--all-features`, `--test-threads=1`. No concurrent Cargo, no live, no GPU
`--ignored` rerun (overlay production unchanged after `1b38f18`; convention
preserved). Logs under that target directory, **not** overwriting the
implementer's RED receipts.

1. `cargo test -p client --all-features --test revision_289_stage2 cleanup_f -- --test-threads=1`
   — `tutorial-input-whole-branch-cleanup_f.log`: exit 0, **13 passed**.
2. `cargo test -p client --all-features --test revision_289_stage2 --test revision_289_outbound --test input --test hud --test minimenu --test walk -- --test-threads=1`
   — `tutorial-input-whole-branch-regression.log`: exit 0. hud 86, input 16,
   minimenu 27, outbound 27, stage2 55, walk 6.
3. `cargo test -p client --all-features --test revision_289_stage3 --test do_action --test social --test chat_mode --test game_shell --test login --test logout --test lost_con -- --test-threads=1`
   — `tutorial-input-whole-branch-legacy.log`: exit 0. chat_mode 4,
   do_action 13, game_shell 3, login 10, logout 7, lost_con 4, stage3 23,
   social 23.

Fourteen suites, **304 passed / 0 failed / 0 ignored**. Matches the parent
receipt. No 950-test rerun.

Preserved implementer failure history (not re-run as RED):

- `tutorial-input-red.log`: E0432 wrong `config::{ButtonType, ComponentType}` import.
- `tutorial-input-red-behavior.log`: open tutorial, no message, emitted `[]` vs `[86, 0, 2]`.
- `tutorial-input-red-local.log`: local kind-0 left `None` vs `Some("")`.
- `tutorial-input-stage2.log`: NPC oracle `[252, 0, 7]` vs production MOVE_OPCLICK+OPNPC1; oracle then aligned to Java, not production.

## Remaining root gates (must stay visible)

- Scoped draw correction above (precedence + colour 128), if live click UX
  is about to be used as proof
- Fresh native build after this docs commit (root-owned)
- Bounded live action / logout proof (root-owned). Preserve
  `client-proof-arrow-0310`: arrows on/off/movement only; clicks/logout
  failed; session hit the 300 s deadline. **No live acceptance yet.**
- Authentic 289 game-cache pairing, checksums, real asset/render proof
- Approved live endpoint / credentials / test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Performance acceptance
- Repo hygiene (merge/push/remotes) remains parent Codex

This reviewer did not launch apps/server, touch caches/accounts/remote, edit
production/tests, or push/merge.

## Explicit offline verdict

**OFFLINE ACCEPTED (bounded)** for `codex/revision-289-client` relative to
original published base **`4f2048ea10f75b3bb92ff45610b35ba7313b0308`**, with
production HEAD **`91fcadde87d9566f7844de4d18b8823fa8dd09fe`**.

Meaning:

- R289 LEFT + pending `Option` (including empty) acknowledges once and
  consumes the click; open tutorial without a message no longer swallows
  clicks; right click passes through; 274 stays the base no-op.
- Kind-0 capture is the shared `add_chat` path. Cold 2 / reconnect 15 /
  logout / TUT_OPEN / IF_CLOSE match the cited Java assignment sites for
  `aString4`.
- NPC overlay blink/motion/scene1 freeze evidence at `1b38f18`/`b03e633`/
  `411ef13` is preserved; this delta does not retouch GPU ownership.
- Tutorial **presentation** is **not** accepted: pending-message branch
  order and hint colour 128 remain a material finding and a live-click
  confounder.
- Prior rejections (`0030afb` whole-branch, overlay rounds 1–2) remain history.
- This does **not** authorize merge, live proof, host integration,
  performance claims, or whole-client acceptance.

## Reviewer checks

- [x] Exact full hashes verified; production freeze `91fcadd` before reading code
- [x] Parent `t_08298357` Grok 4.5 approval of that same hash verified
- [x] AGENTS, STATE current top, ack-fix report, primary Java `aString4` sites read
- [x] Cold diff vs `411ef13` and vs base `4f2048e` handle_chat_if_clicks no-op
- [x] Capture/ack/lifecycle/274/widget/NPC/walk bytes source-grounded; NPC RED oracle not production-shaped
- [x] Draw precedence + colour 128 classified against J5267-5382; effect on silent ack stated
- [x] Overlay freeze/epoch path unchanged after `1b38f18`; GPU `--ignored` not re-invoked
- [x] Independent serial 13 + 304 pass / 0 fail / 0 ignored; implementer RED logs retained
- [x] No source implementation edits; this file only
- [x] Offline verdict does not claim live/performance/presentation/whole-client acceptance
