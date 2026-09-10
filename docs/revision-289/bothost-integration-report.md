# Bothost 274/289 client reconciliation

2026-09-10. Plan step 2; source/regression evidence. The initial candidate
record below is followed by post-review corrections at product commit
`d14755da758c64d971c1103b2d7703f6fd8379fb`. Current review acceptance is tracked
in the host campaign's `docs/compat/STATE.md`. This report does not qualify host 289
sessions, scripts, live worlds, Linux/Windows execution, or release publication.

## Inputs and candidate

- Published bothost base: `9b41e6e06b9fd42dc2247fc813a303c3fcb92941`.
- Adopted 289 source: `c18f3a1148e9caee73426e162677328ca64d1a83`.
- Common ancestor: `4f2048ea10f75b3bb92ff45610b35ba7313b0308`.
- Root merge: `4098a50fb4803f5d6ec9131af38d18e0ba6a395f`.
- Initial reconciled product: `cf386ae5c82071ef661f4a1eb4d38e22b41718d3` on
  `codex/bothost-274-289`. The report/evidence commit changes no product files.

The merge retains the published base as an ancestor. Root combined the explicit
`CLIENT_UNPACK_DIR` override with the published `operator_home` handling and
excluded the incoming standalone campaign's obsolete `AGENTS.md`. Its dated
reports remain historical evidence. No 377 source, host dependency, foreign
runtime, bot action API, or second host packet table was imported.

## Preservation and reconciliation

| Boundary | Result and current regression evidence |
|---|---|
| GPU overlays and lazy chrome | Retain the published comparison against the last uploaded scene RGBA/coverage in `GpuBackend::finish`. Remove the redundant Renderer epoch/hash and its pending-upload path. Real NPC blink, movement, clearing, late writers and unchanged frames pass the production GPU test. |
| Scene-state-1 freeze and minimap | Overlay-only updates preserve the held minimap and last 3D FBO. The strengthened production hint test seeds a distinctive minimap, poisons the corresponding CPU chrome pixel, changes the real hint during freeze, checks the held pixel and upload counts, then checks the unchanged follow-up frame. Existing full viewport and minimap regressions also pass. |
| Main modals and overlays | Remove unconditional main-modal/main-overlay atlas forcing; the final scene comparison observes sealed modal RGB and covered pixels. Side/chat modal forcing remains. `sealed_scene_window_uploads_modal_rgb_without_chrome_redraw` and movement/clear regressions pass. |
| Shared and private client state | Adapt both new 289 actor appearance-cache assignments and their test fixture to the published boxed Packet storage. Preserve animation-base Arc sharing, borrowed delays, interface copy-on-write and dynamic sprite recycling. Add one default-274/explicit-274/explicit-289 constructor regression checking shared cache/interface/template Arc identity. |
| 289 protocol and lifecycle | Preserve the source revision selection, packet framing, ordered outbound fixtures, transactional actor decoding, publication, login and session adoption. Source tests cover partial/overlong packets, production login framing/ISAAC, failed cross-revision adoption and reconnect behavior. No timeout or tick-edge changes were added in reconciliation. |
| CPU presentation and input | Preserve source-grounded 289 tutorial/pending-message ordering and 274 legacy behavior. Five tutorial presentation tests pass, as does the explicit composed GPU ground-input test. Full tutorial/audio parity is outside this milestone. |
| Platform and cache paths | Preserve explicit HOME behavior, Windows USERPROFILE fallback and socket support; compose CLIENT_UNPACK_DIR with those home rules. Current macOS checks pass. Native Windows/Linux verification remains pending. |

Incoming files also needed formatting and mechanical lint corrections for the
current toolchain. The large `client.rs` reconciliation diff is predominantly
formatting of incoming 289 code; review it with and without whitespace filtering.

## Verification

Commands ran in this client repository, using the campaign's `target/client`
cache. Raw logs and SHA-256 summaries are under
`docs/revision-289/evidence/bothost-integration/`.

| Check | Current result | Receipt |
|---|---|---|
| Published 274 baseline: `cargo test --locked --workspace` | 774 passed; 0 failed; 0 ignored; actual GPU regression execution | Host `docs/compat/evidence/client-baseline-274/` |
| `cargo fmt --all -- --check` | Exit 0 | `07-cargo-fmt-check.log` |
| `cargo clippy --locked --workspace --all-targets --no-deps -- -D warnings` | Exit 0 | `13-cargo-clippy-pass.log` |
| `cargo test --locked --workspace` | 990 passed; 0 failed; 2 ignored across 76 test summaries | `14-cargo-test-workspace.log` |
| `cargo test --locked -p client -- --ignored` | Both GPU tests passed; 0 failed; 0 ignored | `root-explicit-gpu.log` and timestamped JSON receipt; worker receipt `15-cargo-test-client-ignored-gpu.log` also passes |
| Product-source whitespace check | Exit 0 | Root `git diff --check -- crates` before product commit |

The two default-ignored tests are the production NPC hint/freeze test and
`gpu_ground_input_composed`. Root ran both with `SKIP_GPU` absent; these are
actual GPU results. They are counted once when describing coverage, despite the
worker's additional passing invocation.

Failures are retained: receipts 00/01 are compile failures while adapting boxed
appearance storage. Receipt 02 passed the original hint-only test, which did
not cover the coupled held-minimap defect. Receipt 03 is the meaningful red
combined test (one failure, exit 101); receipt 04 passes after reconciliation.
Clippy receipts 08 through 12 failed; only receipt 13 establishes the clean lint
gate, despite some earlier filenames containing `pass`.

Root independently recomputed result counts and verified all 197 tracked
product/test/Cargo files against the manifest used for host dependency checks.
The client product commit matches those inputs. Host API/host/host-play tests
with `host-play/memory-profile` subsequently passed 441 tests, zero failures,
with seven live tests still ignored. That check found and fixed an existing
host fixture-name collision; its separate change and red/green evidence live
in host `docs/compat/evidence/client-milestone/` and the host milestone report.

## Review handoff and remaining gates

Implementation used Hermes `sol`, actual `gpt-5.6-sol` / `openai-codex`, session
`20260910_093112_c37a15`, run 1077. After product work and checks were saved,
the worker entered an additional delegate review without completing the
configured handoff. Root reclaimed it, verified the saved inputs, committed
the changes, and completed this report. That delegate review is not counted as
the required review. Some worker searches were blocked by tool restrictions;
alternative scoped commands succeeded. No approval policy was weakened.

The same card `t_e8e16acd` must now run profile `reviewer` (actual Grok 4.5).
After acceptance, root freezes a base/candidate/evidence manifest for fresh
whole-client `branchreviewer` (Grok 4.6) and the independent `orch` (Astra)
integration trial. Later host/profile changes require their own appropriate
checks, and the final whole-campaign Grok review remains outstanding.

Current macOS evidence does not reproduce or dismiss historical Windows/Linux
GPU shade and Windows CRC timing failures. Available-platform details are in
host `docs/compat/fixture-inputs.md`. Host profile binding, all direct host
writers, 289 nav/content, and local live acceptance remain later plan gates.
The host public gitlink and remote branches have not been changed.

## Post-review corrections

The initial candidate `8b1a80918f87089e4eb339f1d0e216c1b163348d` received the
same-card Grok 4.5 approval and a fresh Grok 4.6 whole-client approval. The
independent Astra review found a P1 lifecycle regression and reproduced it with
debugger state injection. Root accepted that concrete finding; the earlier
approvals do not establish acceptance of the defective candidate. Original
independent reports, actual model receipts and root reconciliation are in host
`docs/compat/reviews/`.

Corrected product: `d14755da758c64d971c1103b2d7703f6fd8379fb`.

- Clear the reusable inbound Packet's old game-frame bound when login begins
  reading its eight-byte server seed. Game-packet bounds remain in force. A new
  socket regression receives a real one-byte run-energy packet before cold
  relogin, reconnect or socket adoption, for both 274 and 289. It reproduces the
  bounded `g8` panic before the fix and verifies successful handshakes and
  retained reconnect state after the fix.
- Root also found that incoming report-button handling completed previously
  idle 274 controls. Restrict the new mute and reason handlers to 289. A new
  regression verifies idle 274 controls, the 289 mute toggle, and all twelve
  ordered 289 report-reason frames. This test also fails before the correction.

Correction receipts are `docs/revision-289/evidence/bothost-corrections/`:

| Check on the corrected product | Result |
|---|---|
| Seven affected login/adoption/packet/revision suites | 127 passed, 0 failed, 0 ignored |
| Full `cargo test --locked --workspace` | 992 passed, 0 failed, 2 ignored GPU tests |
| Explicit `cargo test --locked -p client -- --ignored` | Both GPU tests passed, 0 failed, 0 ignored |
| Formatting and strict all-target Clippy | Exit 0 |

The full workspace and explicit GPU receipts name the exact committed product.
`summary.json` records raw-log hashes and independently recomputed counts.
`00-login-test-compile.log` records a test-helper compile error; it is distinct
from the subsequent runtime failure in `login-bound-red.log`. Both meaningful
red logs are retained. Corrective Grok 4.5 and Grok 4.6 review must cover this
product before root accepts the integration milestone.
