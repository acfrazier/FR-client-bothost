# Cleanup F: staged social, chat, and player options

Task: t_4583da40. Foundation: 2a0f774 (accepted Cleanup E). Branch:
codex/revision-289-client. This report covers only the F-owned inbound rows 13,
21, 47, 168, 196, 235, and 243. The R274 dispatcher and public protocol tables
remain unchanged; R289 is selected only by the explicit session revision.

## Implementation

`crates/client/src/client/client.rs` extends the bounded R289 decode -> apply ->
outcome publication path with typed social operations:

- `CHAT_FILTER_SETTINGS` (13) validates and consumes all three mode bytes,
  updates the mode state, marks both chat redraw obligations, and publishes a
  declared chat generation even without adding a log line.
- `SET_PLAYER_OP` (21) preserves null clearing, accepts only indices 1..5, and
  preserves the priority bit semantics.
- `UPDATE_IGNORELIST` (47) validates a frame remainder divisible by eight,
  enforces the 100-entry capacity before mutation, and stages all p8 hashes.
- `UPDATE_FRIENDLIST` (168) stages the fixed p8+p1 fields, then reuses the
  existing friend ordering/login/logout behavior and reports chat publication
  only when a notice is actually added.
- `MESSAGE_GAME` (196) handles trade, duel, and challenge request suffixes;
  kind-0 messages are stored in tutorial state and clear the tutorial click
  latch when the tutorial chat is open.
- `FRIENDLIST_LOADED` (235) updates status and side redraw state.
- `MESSAGE_PRIVATE` (243) rejects headers shorter than 13 bytes, stages the
  exact framed compressed body, preserves deduplication/ignore/staff gates,
  and publishes chat only when a line is added.

`Client` now retains tutorial chat text and clears it with the existing logout
modal reset. Named constants for the two previously numeric F rows were added
to `ServerProt289`.

## Verification

All commands used the isolated target directory:

- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo check -p client` — PASS.
- `cargo test -p client --test revision_289_stage2 -- --test-threads=1` — PASS, 48/48.
- `cargo test -p client --lib --tests` — PASS, including 73 library tests and 48 stage2 tests.
- `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1` — PASS in the isolated workspace; includes library 76, stage1 46, stage2 48, stage3 23, server_packets 19, zones 8, legacy zone 45, renderer suites, social 23, and client-play 4.
- `python3 tools/verify_revision_289_contract.py` — PASS: 256 inbound, 82 outbound rows, 50 fixtures.
- `git diff --check` — PASS.

The first aggregate client test invocation also reproduced the known
load-sensitive `fixed_empty_sync_after_welcome_and_unknown_frame_reset_once`
stage1 failure (1/46); the required isolated serial stage1 rerun passed 46/46,
and the subsequent all-features workspace run passed.

## Remaining gates

This is offline packet/lifecycle proof only. Authentic cache/server pairing,
live RSA/ISAAC/presentation compatibility, and the required final whole-branch
Grok4.6 review remain root gates. Existing untracked campaign review/helper
artifacts were preserved and are outside this F change.

## Review correction coverage

The challenge-request branch now follows primary Java J:3264-3276: it uses the
body between the first colon and the nine-byte `:chalreq:` suffix, with the
player name as sender. Tutorial kind-0 text is rendered over the tutorial chat
interface with `Click to continue`, and a latched click clears the text and
requests chat redraw without closing the interface.

Production-path fixtures in `revision_289_stage2` additionally cover challenge
body and tutorial acknowledgement, all three request suffixes, ignored private
exact consumption with a nonzero message ID and WordPack body, private
deduplication, every staff level and crown/type gate, ignore-list non-divisible
remainder and capacity rejection without mutation, private short-header
rejection, friend add/same-world/login/logout/current-world sorting and notice
generations, null/out-of-range/edge player-option indices and priority, and a
malformed later field. Focused execution passed 6/6 tests; the expected
fail-closed T2 diagnostics were emitted for malformed frames.
