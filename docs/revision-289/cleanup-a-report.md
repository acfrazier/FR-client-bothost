# Cleanup A report: corrected frame and lifecycle foundation

Task t_358c02ad, correction after rejected 203f176 and c135f60. Same-card
independent Grok4.5 review is required; this is implementation evidence, not an
approval or live acceptance. The earlier report overstated generic admission
and semantic publication. This report supersedes those claims, not the rejection
receipts. Approved contract: 0b89fc3, design decisions 1-8 and section A.

## Implementation and source evidence

- `client.rs:297-386`: internal `R289Operation` contains validated UpdatePid,
  Logout, or LastLoginInfo data. The decoder reads the whole exact frame before
  apply. `R289Outcome` distinguishes Applied(generation set) from Reset;
  `R289Publication` represents each family once, not raw packet IDs.
- `client.rs:3811` (`handle_packet`): R289 publishes the returned set once.
  T1, caught T2 and in-band actor logout return/reset through lifecycle, with no
  additional success-family publication. The private reset marker bridges old
  handlers that call logout internally; it is not a generation comparison.
  The R274 dispatcher and numeric publication match retain their established
  behavior. `bump_gens` now explicitly rejects R289 callers: there is no second
  R289 numeric publication table, including in the old public test helper.
- `client.rs:6876` (`dispatch_packet_289`): generic fixed-size admission rejects
  short/long declared frames before field reads, including nonempty payloads on
  zero-size IDs. This does not imply that every table ID has a supported operation.
  Unconverted B-H handlers return their previous family effects directly from
  their dispatch arms. No missing B-H generation families were added here.
- Existing production `read_packet` clears the old frame stamp at a new header,
  waits for partial headers/payloads, and stamps the declared end, including zero.
  `client.rs:11480` (`inbound_end`) prioritizes that stamp. Socket-free historical
  callers without a stamp can still use nonzero psize or an exact vector when
  psize is unset; oversized/negative explicit R289 lengths no longer silently
  fall back to allocation length. Tests injecting a zero frame explicitly stamp
  zero. Production never treats zero psize as allocation length.
- `io/packet.rs` retains byte primitive bounds and now rejects an unterminated
  string in a stamped frame, including empty frames. Unstamped cache/file reads
  retain their previous behavior. Strict strings exposed an old stage2 fixture
  that declared 8 bytes for the 9-byte option including terminator; its declaration
  now comes from the actual payload length, with an independent malformed-tail
  production regression rather than weakening the check.
- `client.rs:4290,4310`: Section A apply is packet-free. UPDATE_PID applies its
  staged u16 slot/u8 membership with no family bump (ledger row120). LOGOUT calls
  existing lifecycle and returns Reset (row121). LAST_LOGIN reads i32/u16/u8/u16/u8,
  selects 655 for recovery!=201 OR warning==1 and 650 otherwise, and uses the
  matching component's layer rather than component ID. Welcome/report effects
  are gated by nonzero IP and no main modal. Close-only welcome also returns iface
  even if no matching component exists; closed gates return an empty set.
- Primary Java was read directly: client.java:2648-2653,2833-2837,3159-3183;
  Class1_Sub1_Sub3:259-293 confirms ordinary big-endian reads; Class6:229-233
  identifies self ID, layer and clientcode. Pin/absolute aliases are those in
  `full-dispatch-audit.json`, rows120/121/253. Synthetic fixtures are independently
  specified from these field widths and branches, not live/private payload captures.
- The welcome path now follows Java method110 (2282-2301), not incoming IF_CLOSE:
  emits existing revision-mapped CLOSE_MODAL93, closes side/chat, retains the count
  dialog, and clears pause only when a side/chat modal was closed. This is the
  existing outbound operation at the Section-A lifecycle callsite, not section-H
  telemetry implementation. No renderer-owned resources are touched.
- Last-login values and optional DNS display storage are lifecycle-owned and
  reset in `logout` (`client.rs:8580`). A new notice clears stale DNS display.
  No DNS worker/resolution was added; optional display remains None unless supplied
  by lifecycle code. There is no packet-time network lookup or login-IP logging.

## Production-path regression evidence

`crates/client/tests/revision_289_stage1.rs` now covers:

- `all_fixed_sizes_reject_short_and_long_before_decode_on_production_dispatch`:
  drives actual handle_packet for all fixed entries; checks cursor zero, retained
  PID/report state and all-family reset once. Unsupported entries are tested for
  admission only, not claimed implemented.
- Every short LAST_LOGIN bound plus an overlong bound on a reusable allocation:
  no field reads, report clears or outbound acknowledgement before reset.
- Full six-case recovery/membership truth table, distinct component/layer IDs,
  IP-zero and existing-main-modal gates, close without a welcome match, redraws,
  method110 acknowledgement/count-dialog/pause semantics, and exact generation sets.
- Fragmented LAST_LOGIN, adjacent PID, fixed empty VARP_SYNC and LOGOUT; partial
  fixed body stays pending without mutation. Fragment helper uses a per-chunk
  barrier, not sleep timing, to prevent the next chunk arriving before polling.
- Variable-g1/g2 zero frames after a valid longer welcome; valid empty ignorelist;
  partial g2 header; unterminated option with a previous frame's newline in the
  reused tail; valid-prefix state retained after malformed inventory/option frames.
- T1 unsupported fixed ID122 and in-band PLAYER_INFO mismatch invalidate all once.
  The latter was also run with `--nocapture`: actual diagnostic was the
  getplayer size-mismatch T2, not a helper-only failure.
- Default274 socket LAST_LOGIN remains a no-op, adjacent legacy PID retains its
  meaning, and existing inventory width/padding/default revision tests pass.

A fixed socket packet has no declared-length field: short bytes mean pending,
and extra bytes are the next opcode. Accordingly, impossible short/long fixed
lengths are injected at the production handle_packet boundary, while socket
tests prove pending reads, exact fixed boundaries and adjacent frames. These are
not advertised as malformed fixed length headers that do not exist on the wire.

## Executed checks

All commands ran in this client checkout with
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`.

- `cargo test -p client --test revision_289_stage1 --test revision_289_stage2
  --test revision_289_stage3 --test packet --test gens --test logout
  --test server_packets`: passed. Counts respectively 46,36,23,7,12,7,19.
  Log: `target/cleanup-a-regressions.log`.
- Same command with `--all-features`: passed with the same counts, no warnings.
  Log: `target/cleanup-a-all-features.log`.
- `cargo test -p client --lib --test do_action --test walk --test prot
  --test from_shared --test player_info --test login_rsa --test iface_model
  --test rebuild -- --test-threads=1`: passed. Lib73, do_action13, walk6, prot2,
  from_shared6, player_info1, login_rsa2, iface_model9, rebuild12.
  Includes `freeze_last_scene_only_while_the_game_is_loading` and frozen modal
  rendering tests. Log: `target/cleanup-a-preservation-corrected.log`.
- `cargo check -p client -p client-play --all-features`: passed.
- `python3 tools/verify_revision_289_contract.py`: PASS 256 inbound, 82 outbound,
  50 fixtures. Structural verifier is not proof of all70 semantic completion.
- `git diff --check`: passed. Touched new code formatted; unrelated pre-existing
  client.rs/stage2 formatting differences were not swept into the patch.

Failures retained as evidence: new close-only, overlong LAST_LOGIN, method110 emit,
raw-R289-helper rejection and oversized-storage tests failed before their fixes;
zero MESSAGE_GAME initially exposed unterminated-string acceptance. The first
preservation command named nonexistent integration target `freeze_last_scene`
and did not run tests; the corrected command runs its actual lib test. No failed
command is counted as a pass.

## Remaining boundary

Only A has the new fully decoded semantic-operation representation. Existing
inventory staging remains intact. B-H are not accepted or fully atomic: actors
still have bit-read/frame and mutation-before-validation gaps (`gbit` still uses
the backing allocation), enclosed zones still need all-inner validation, and
the other missing handlers, field semantics, generation/redraw obligations and
outbound operations remain as documented in the approved plan. Byte bounds are
not a claim of bounded/atomic actor or enclosed-zone decoding.

No host/server/vault writes, live sessions, remotes, merges or pushes. Inherited
untracked review/tracer files are preserved and excluded from this commit.
Authentic cache/server pairing, approved endpoint/account, live RSA/ISAAC and
presentation proof, plus final Grok4.6 whole-branch review, remain root-owned.
