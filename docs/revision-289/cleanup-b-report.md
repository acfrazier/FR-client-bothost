# Cleanup B report: bounded interface semantics and tutorial publication

Task t_36cd00a5. This implementation extends the internal `R289Operation` decoder/apply seam from Cleanup A; it does not route raw R289 IDs through the R274 dispatcher.

## Implemented

- Added source-named R289 interface operation variants and revision constants for IDs 12, 18, 23, 30, 35, 55, 59, 63, 79, 81, 119, 127, 138, 160, 181, 184, 189, 211, 222, 244, and 252.
- Decodes the entire bounded frame before interface mutation, including terminated text and signed fields.
- Applies interface changes through the per-client `IfTypeMut` overlay and returns one `iface` publication per operation.
- Corrects IF_SETTAB 65535 to -1, IF_SETOBJECT 65535 to model-type clear, signed position/sequence/overlay fields, scroll clamp ordering, colour/model/head writes, tutorial flash/open, count-dialog state, and modal transitions.
- Resets child animation state for single-modal IF_OPENCHAT/IF_OPENMAIN/IF_OPENSIDE while preserving Java's no-reset IF_OPENMAIN_SIDE behavior; transformed local-player heads select the transmog model path.
- Keeps rendering ownership unchanged; no renderer resources or public bot action API were added.

Primary evidence: `full-dispatch-cleanup-plan.md` Section B and `full-dispatch-audit.json` rows listed above. Existing Java anchors remain the source authority; no private payloads were copied.

## Verification

- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo check -p client` — passed.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --lib --test revision_289_stage1 --test revision_289_stage2 --test revision_289_stage3` — passed in isolated serial runs: lib 73, stage1 46, stage2 40, stage3 23. Stage2 includes exact production-dispatch coverage for all 21 B rows, malformed-frame reset, modal reset distinction, transmog head, and two-client overlay isolation.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --test revision_289_stage2 cleanup_b_ -- --test-threads=1` — passed: 3 focused tests.
- `git diff --check` — passed.

## Remaining C-H

Cleanup C inventory/varps/stats, D actors, E direct and strict enclosed zones, F social/chat tutorial kind0 and challenge/SAY semantics, G camera/audio/hints/HUD, and H outbound coverage remain root-dispatched work. The Section-F future kind0 tutorial-message seam is not implemented here. Authentic cache/server pairing, live RSA/ISAAC/login, presentation proof, and final Grok4.6 review remain external/root gates.
