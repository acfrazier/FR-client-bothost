# Cleanup C report: bounded inventory, variables and stats

Status: implementation complete pending same-card reviewer approval.

## Scope and source contract

Section C owns inbound IDs 28, 46, 75, 76, 97, 107, 154, 172 and 195.
The primary source anchors are recorded in `full-dispatch-audit.json`:

- `J:2745-2754` — UPDATE_INV_STOP_TRANSMIT (28): clear final item IDs while preserving counts.
- `J:2629-2636` — UPDATE_RUNWEIGHT (46): signed i16 weight and stats-tab redraw.
- `J:3402-3416` — VARP_SMALL (75): u16 varp ID and signed i8 value.
- `J:3477-3495` — UPDATE_INV_PARTIAL (76): uSmart slot, u16 object, count byte or i32 extension.
- `J:3526-3540` — VARP_LARGE (97): u16 varp ID and i32 value.
- `J:2972-2991` — UPDATE_INV_FULL (107): u16 component, u16 count, extended counts.
- `J:2775-2790` — UPDATE_STAT (154): stat, i32 XP, effective level, derived base level.
- `J:3351-3361` — VARP_SYNC (172): reconcile server/current vars and invoke client-var effects on change.
- `J:2621-2628` — UPDATE_RUNENERGY (195): energy byte and stats-tab redraw.

## Implementation

`crates/client/src/client/client.rs` now decodes all nine rows into bounded
`R289Operation` variants before any state mutation. Inventory full and partial
entries are staged by the decoder, exact frame consumption is asserted, and
publication is returned as the appropriate inventory, varp or stat family.
The R289 path keeps the 274 dispatcher independent.

- Full inventory uses the R289 u16 count and clears trailing slots.
- Partial inventory uses uSmart slots, consumes out-of-range slots without
  storing, and preserves sparse slots beyond 127 and 255.
- Stop-transmit clears only item IDs; counts remain unchanged.
- Small varps use signed i8 values, large varps use i32 values, and sync
  reconciles `var_serv` without zeroing missing client entries.
- Changed small/large varps dirty tutorial chat only when `tut_com_id` is open;
  unchanged values and VARP_SYNC reconciliation do not add that redraw.
- Stats derive base level from XP while retaining the wire effective level.
- Run weight uses signed i16 and run energy uses the wire u8; both publish the
  stat family.
- R289 inventory frames with declared zero length do not fall back to reusable
  backing allocation bytes.

`crates/client/src/io/revision.rs` names opcode 28 as
`UPDATE_INV_STOP_TRANSMIT`; the existing canonical R289 size table remains the
source of fixed/variable frame lengths.

## Exact tests

Production-path tests added in
`crates/client/tests/revision_289_stage2.rs`:

- `cleanup_c_all_rows_use_bounded_production_apply_and_publication`: exact
  bytes for all nine C rows, extended count, signed varp/weight, tutorial
  redraw/no-redraw (including VARP_SYNC no-chat redraw), varp generation
  advancement for small/large/sync, derived stat level, family publication,
  sparse slots 127/128/255/256, out-of-range consumption, empty full
  inventory, and stop-transmit count preservation with no side redraw.
- `cleanup_c_inventory_rejects_stale_zero_and_truncated_frames_atomically`:
  stale backing data on a declared zero frame and a truncated later full entry
  cannot publish staged inventory data.

Verified commands, isolated to the required target directory:

- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo check -p client` — PASS.
- `... cargo test -p client --test revision_289_stage1 -- --test-threads=1` — PASS, 46/46.
- `... cargo test -p client --test revision_289_stage2 --test server_packets -- --test-threads=1` — PASS, 42/42 and 19/19.
- Focused `cleanup_c_` run — PASS, 2/2.

The serial stage1 failure observed while combining suites was the pre-existing
`fixed_empty_sync_after_welcome_and_unknown_frame_reset_once` load-sensitive
flaky test; its isolated run is 46/46 and the exact test passes in the final
isolated stage1 command above.

## Remaining gates

This report does not claim live server/cache pairing, RSA/ISAAC/login
compatibility, GPU presentation, whole-client acceptance, or final Grok4.6
branch approval. Sections D-H remain outside this card. Existing unrelated
formatting differences make whole-repository `cargo fmt --check` report
pre-existing diffs; no formatter-wide rewrite was applied.
