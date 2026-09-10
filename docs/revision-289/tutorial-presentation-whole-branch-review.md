# Revision 289 tutorial presentation: required Grok 4.6 whole-branch review

Task: `t_e73008ec`
Role: required final `branchreviewer` (not a section review, not live/release authorization)
Model: grok-4.6
Provider: xai-oauth (profile defaults; no task model/provider override)
Worker session: `20260909_070122_467319`
Kanban run: 957
Date (local): 2026-09-09 EDT

## Verdict: OFFLINE ACCEPTED (bounded)

Independent Grok 4.6 whole-branch review of `codex/revision-289-client` after
the tutorial pending-message presentation correction. This is **not**
whole-client acceptance, **not** live action/logout proof, **not** authentic
cache pairing, **not** hardware GPU presentation, and **not** performance
acceptance.

The material remaining presentation finding from `t_7fe3337b` (report
`6475746`) is **closed** in the combined branch at production HEAD `03d916a`.

Predecessor receipts are **preserved**, not rewritten:

- `t_77d35bfb` REJECTED `0030afb` — `docs/revision-289/branch-review.md`
- `t_615aa9ac` OFFLINE ACCEPTED (bounded) `0e3b6a7` — `docs/revision-289/branch-rereview.md`
- `t_f4e2ad64` OFFLINE ACCEPTED (bounded) `fc5516c` — `docs/revision-289/branch-final-review.md` (still untracked in this tree; identity kept)
- Cleanup A–H code `0406ceb` aggregate Grok 4.5 `e4834d7` and whole-branch Grok 4.6 `0227f3f` (`t_d1a06f94`)
- NPC overlay `1b38f18` + GPU-test portability `b03e633` + report `411ef13` (`t_e7dfd2e6`)
- Input correction `91fcadd` same-card APPROVED (`t_08298357`) and whole-branch OFFLINE ACCEPTED (bounded) `t_7fe3337b` / docs `6475746`, **expressly excluding** pending-message presentation
- Parent same-card `t_ea8cdf17` APPROVED `03d916a` (actual Grok 4.5 / xai, artifact lens; handoff captured on this card)

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
| Input-reviewed production | `91fcadde87d9566f7844de4d18b8823fa8dd09fe` |
| Input whole-branch docs | `647574624ab0908e2b58d45821082c78236ce6a6` |
| Parent-reviewed production HEAD | `03d916afe29808d76709f4ff91152059da6dfcd2` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| `CARGO_TARGET_DIR` | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by this reviewer | none |
| Follow-up cards created | none |

Code freeze check (this run):

- `git rev-parse HEAD` equals `03d916afe29808d76709f4ff91152059da6dfcd2`.
- `git status --porcelain` has **no tracked modifications**. Inherited untracked non-deliverables left unstaged: `docs/revision-289/branch-final-review.md` and five `tools/*` tracers.
- Parent Grok 4.5 reviewed this same full hash. No source change during the usage pause.
- `git diff --stat 91fcadd..HEAD` is the presentation correction plus the already-landed input whole-branch report: `cpu.rs`, `draw.rs`, `tutorial_presentation.rs`, `STATE.md`, `tutorial-presentation-fix-report.md`, `tutorial-input-whole-branch-review.md`.
- Overlay GPU/renderer files are unchanged after `1b38f18` except the already-reviewed ignore-metadata in `gpu_overlay_tests.rs` (`b03e633`). `freeze_last_scene` remains `kind == Game && scene_state == 1`.
- `handle_chat_if_clicks` remains the `91fcadd` R289 + LEFT + `is_some()` ack. `add_chat` kind-0 capture is unchanged.

Span `4f2048e..HEAD`: 55 commits.

`docs/revision-289/STATE.md` current top at this freeze still describes the
presentation card as awaiting same-card review. That lag is the implementer's
`03d916a` snapshot; this reviewer does not edit STATE. Parent `t_ea8cdf17`
already completed APPROVED on this hash.

## Scope read

- Local `AGENTS.md`, `docs/revision-289/STATE.md` current top, `tutorial-presentation-fix-report.md`, `tutorial-input-whole-branch-review.md`.
- Primary 289 Java (read-only): `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/client.java` — method129 5261-5386 (chatback, social, amount, `aString4`, modal, ordinary/tutorial IF), per-frame dirty 9832-9840, LEFT ack 6042-6046.
- Production: `Renderer::draw_chat`, `CpuBackend::begin`/`chrome`, `GpuBackend::begin`/`chrome`, `freeze_last_scene`, `handle_chat_if_clicks`, `add_chat`, `PixFont::centre_string`/`draw_string`/`string_wid`, `Colour::DARKBLUE`.
- Tests: `crates/client/tests/tutorial_presentation.rs` (synthetic fonts/sprites/IFs; independent pixel oracle).

Lens: artifact-first cold read of `draw_chat` branch order, colours, per-frame
chat dirtiness, GPU flag capture, freeze/overlay non-touch, then comparison to
the parent handoff and to the `t_7fe3337b` finding, then independent serial
execution. Full workspace 950-test suite was **not** repeated. GPU `--ignored`
overlay proof was **not** re-invoked (overlay production unchanged; no hardware
GPU claim).

## Combined presentation correction (primary attention)

### Prior finding (`t_7fe3337b`)

Rust `draw_chat` after social/amount let chat modal win, then drew the tutorial
IF and overlaid both message and `"Click to continue"` in `Colour::BLACK`, and
skipped the hint entirely when `tut_com_id == -1`. Chat was dirtied only on
capture/ack. That mismatched method129 J5267-5382 (pending non-null replaces
modal/tutorial/plain chat; message colour 0; cue colour 128) and J9832-9834
(dirty chat every pending frame). It also made leftover pending acks invisible.

### Draw order vs method129

Java chat-area order (5267-5382), after plotting chatback (5266):

1. social prompt
2. enter-amount prompt
3. **`aString4 != null`**: centre message colour **0** at (239,40), then
   `"Click to continue"` colour **128** at (239,60); do not draw chat modal or
   tutorial IF
4. else chat modal (`anInt408`)
5. else if `anInt271 == -1`: ordinary chat
6. else: tutorial IF only

Rust `draw_chat` (3024-3082) after this commit:

1. social prompt (second line already `Colour::DARKBLUE` = `0x80`)
2. enter-amount (second line `DARKBLUE`)
3. **R289 && `tut_com_message.is_some()`** (including empty): message
   `Colour::BLACK` (0) at (239,40), cue `Colour::DARKBLUE` (128) at (239,60)
   on chatback; no modal / tutorial IF / ordinary chat
4. else chat modal
5. else if `tut_com_id != -1`: tutorial IF only (cleanup overlay removed)
6. else ordinary chat

`Colour::DARKBLUE` is `0x80` / 128 (`graphics/colour.rs`). Social/amount already
used that second-line colour; the continue cue now matches J5275.

Empty `Some("")` still selects the branch (`is_some()` / Java non-null). Missing
`b12` skips glyph plot the same way social/amount do; the synthetic suite
injects a font and asserts the absent-cache path.

R274 does not enter the pending branch. An injected 274 `Some(_)` keeps base
modal/tutorial/plain order. That restores pinned base `4f2048e` draw (no
pending overlay), not the cleanup-era overlay.

### Per-frame chat dirtiness vs J9832-9834

Java 9832-9840 sets `aBoolean74` while `aString4 != null`, then `method129`,
then clears the flag.

Rust publishes `redraw_chat = true` in shared `CpuBackend::begin` when R289 and
`tut_com_message.is_some()`, after `prepare_game` and before the
`redraw_frame` chrome-strip path. `CpuBackend::chrome` still draws/clears
`redraw_chat` at the existing site (639-641). The pending assignment does not
set `redraw_frame`, `redraw_side`, `redraw_icons`, or `redraw_chat_mode`.

This is a scheduling-stage difference from Java's late dirty, with the same
chat effect: chrome consumes the flag the same frame. No stage between begin
and chrome clears chat dirtiness.

### GPU propagation (source audit, not hardware)

`GpuBackend::begin` delegates to `self.cpu.begin` (`gpu.rs` 1188), so the
pending flag is published before GPU chrome. GPU chrome captures
`redraw_side || redraw_chat || redraw_icons || redraw_chat_mode` into
`atlas_dirty` / `chrome_upload_pending` (`gpu.rs` 1395-1400) **before**
`self.cpu.chrome` clears `redraw_chat`. Putting the flag only inside CPU
chrome would miss that capture.

Chrome atlas upload on `redraw_chat` is the existing whole-chrome GPU policy,
not a new scene/overlay invalidation. `overlay_upload_needed` remains
epoch-keyed. `freeze_last_scene` is unchanged. `gpu.rs` / `renderer.rs` /
overlay tests are not in `03d916a`.

This is **not** an executed GPU upload-count or adapter proof. GPU `--ignored`
was not run; that is an explicit non-claim, not a silent skip.

### Input / 274 / overlay preservation

- Ack remains R289 + LEFT + `is_some()` (`client.rs` 5881-5892; Java 6042-6046).
  Presentation now matches that condition: leftover pending after IF close is
  visible as the continue prompt, so a subsequent LEFT is no longer a silent
  ack of an invisible message.
- Kind-0 capture in `add_chat` is unchanged.
- NPC overlay blink/motion/signature and scene1 last-FBO freeze are not in this
  delta.
- 274 capture/ack/draw stay the base no-op / base interface order.

Pre-existing handler-order difference vs Java 6030-6049 (Rust tabs then ack
then walk/`mouse_loop`; Java walk then ack) is unchanged from `91fcadd` and is
not a presentation defect.

## Tests actually run (this reviewer)

Serial, this checkout, `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`,
`--all-features`, `--test-threads=1`. No concurrent Cargo, no live, no GPU
`--ignored` rerun. Implementer RED receipts under that target directory were
**read, not overwritten**.

1. `cargo test -p client --all-features --test tutorial_presentation -- --test-threads=1`
   — exit 0, **5 passed / 0 failed / 0 ignored** in 0.26s.
2. `cargo test -p client --all-features --test tutorial_presentation --test hud --test input --test revision_289_stage2 --test revision_289_stage3 --test revision_289_outbound --test minimenu --test chat_mode -- --test-threads=1`
   — exit 0. chat_mode 4, hud 86, input 16, minimenu 27, outbound 27, stage2 55,
   stage3 23, presentation 5.

Eight suites, **243 passed / 0 failed / 0 ignored**. Matches the parent
receipt. No 950-test rerun. Existing 304-test input/HUD receipt from `t_7fe3337b`
remains preserved evidence for unchanged input code; this pass re-ran the
affected presentation/input/289/HUD subset named by the card.

### Pixel oracle (independent of test names)

Synthetic font: 1×1 mask value 1, advance 2, height 1, zero offsets. Oracle
reimplements `centre_string` (`x - string_wid/2`) plus `draw_string`
(`y -= height`, skip plot on space, advance anyway). For ASCII that is
`start = 239 - len` and glyph x `start + 2*i` at row `baseline-1`. Expected
colours are literals `0` and `128`, not `Colour::*`. Chatback index 1 maps to
palette `0xabc123`. Composite check is blit at (17, 357). Fixture asserts the
cache dir does not exist; `Renderer::new(false)` + `game_draw` hits production
`draw_chat` / CPU begin/chrome. `scene_state = 1` is the loading-frame path
(no world raster), which is also the freeze predicate; chrome still draws chat.

Literal `"Click to continue"` is 17 ASCII characters. Synthetic `char_advance`
2 gives `string_wid` 34; centred first glyph x is `239 - 17 = 222`; cue
baseline 60 minus height 1 is row 59. That is the documented colour-mutation
coordinate, from the source string and the test font, not a buffer/off-by-one
guess.

Preserved implementer failure history (not re-run as RED; hex checked here):

- `tutorial-presentation-red-precedence.log` / `red-behavior.log`: fixture
  compile / `Arc` get_mut. Not behavioral.
- `tutorial-presentation-red-pixels.log`: chat pixel (0,0) **1193046**
  (`0x123456` tutorial IF) vs **11256099** (`0xabc123` chatback). Old
  production still composited the tutorial rectangle.
- `tutorial-presentation-red-dirty.log`: chat pixel (0,0) **15658734**
  (`0xeeeeee` poison) vs **11256099**. Missing per-frame dirty left the
  poisoned surface.
- `tutorial-presentation-red-colour-mutation.log`: chat pixel (222,59)
  **0** vs **128** — first glyph of the 17-character cue at the derived
  centre x/baseline above. Isolated BLACK cue after precedence+dirty
  already green.

Current independent 5/5 run would not pass those three behavioral REDs.

## Remaining root gates (must stay visible)

- Fresh native build after this docs commit (root-owned)
- Bounded live action / logout proof (root-owned). Preserve
  `client-proof-arrow-0310`: arrows on/off/movement only; clicks/logout
  failed; session hit the 300 s deadline. **No live acceptance yet.**
- Hardware GPU chrome-upload / adapter presentation of the pending prompt
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
production HEAD **`03d916afe29808d76709f4ff91152059da6dfcd2`**.

Meaning:

- R289 pending `Some` (including empty) now precedes chat modal / tutorial IF /
  ordinary chat, after social/amount. Message is black at (239,40); continue
  cue is DARKBLUE 128 at (239,60) on chatback.
- Shared CPU begin marks only chat dirty each pending frame; GPU chrome can
  observe that flag before CPU chrome clears it. Scene/overlay signatures and
  scene1 last-FBO freeze are untouched.
- Input `91fcadd` (LEFT ack of pending only; 274 no-op; kind-0 capture) remains
  intact. Presentation and acknowledgement now agree, so leftover pending after
  tutorial close is a visible continue prompt rather than a silent LEFT steal.
- NPC overlay evidence at `1b38f18`/`b03e633`/`411ef13` is preserved.
- The `t_7fe3337b` material presentation finding is **closed** offline.
- Prior rejections (`0030afb` whole-branch, overlay rounds 1–2) remain history.
- This does **not** authorize merge, live proof, host integration,
  performance claims, hardware GPU presentation, or whole-client acceptance.

## Reviewer checks

- [x] Exact full hashes verified; production freeze `03d916a` before reading code
- [x] Parent `t_ea8cdf17` Grok 4.5 approval of that same hash verified
- [x] AGENTS, STATE current top, presentation-fix report, prior input whole-branch review, primary Java method129 / 9832 / 6042 read
- [x] Cold `draw_chat` order vs J5267-5382; colours 0/128; empty Some; 274 gate
- [x] Shared begin dirty vs J9832-9834; GPU begin/chrome capture path source-audited; freeze predicate unchanged
- [x] Input ack/capture unchanged vs `91fcadd`; overlay files unchanged after `1b38f18` except `b03e633` ignore metadata
- [x] Synthetic pixel oracle independently checked against PixFont; RED logs hex-checked (IF 0x123456, poison 0xeeeeee, cue 0 vs 128)
- [x] Independent serial presentation 5/5 and 8-suite 243/0/0; no 950 rerun; GPU `--ignored` not invoked (explicit non-claim)
- [x] No source implementation edits; this file only
- [x] Offline verdict does not claim live/performance/GPU-hardware/whole-client acceptance
