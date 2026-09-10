# Offline fixture isolation: stage2 and cleanup E

Task: t_0c645ab5
Branch: codex/revision-289-client
Scope: test fixtures and evidence only; no production code, timeout, validator, cache, server, live or H changes.

## Root cause

The inherited positive fixtures constructed clients with `cache_dir=/tmp`. On a cold machine, `Client::load_cache` correctly fell back to `Cache::default`, whose config tables are empty. The production R289 zone validator then correctly rejected positive references before applying them:

- stage2 `startup_289_enclosed_zone_uses_289_inner_opcodes_and_keeps_framing` used LOC_ADD_CHANGE ID `0x1234` although the cold cache had no loc table entry. The assertion subsequently observed `(zone_update_x, zone_update_z) == (0, 0)` after the fail-closed T2 path instead of `(40, 48)`.
- cleanup E `revision_289_zones` used positive loc IDs 5, sequence ID 7, spotanim IDs 1/3 and object ID 0 while the cold cache had empty tables. The exact failures were `invalid zone loc`, `invalid loc sequence`, `invalid zone spotanim`, and `invalid zone object`.
- The validator was not weakened and no production behavior was changed. Existing negative enclosed-frame cases still use out-of-range IDs and continue to assert fail-closed reset/no publication.

The prior stage1 welcome/logout bounded socket-poll concern was not changed; the final all-features run still passed stage1 46/46.

## Fixture correction

- `revision_289_stage2.rs` now gives each client a unique, deliberately nonexistent temporary cache path based on PID and nanoseconds, so ambient `/tmp` cache state cannot affect the test. The enclosed framing positive now references synthetic loc ID `0`, and the test explicitly seeds the smallest valid synthetic loc table (`len=1`). The frame remains two real R289 inner operations (LOC_ADD_CHANGE 90 and OBJ_DEL 71), consumes all 11 bytes, and asserts origin, exact framing and ingame state.
- `revision_289_zones.rs` uses the same unique nonexistent-cache convention. Its client fixture explicitly seeds only the definitions required by positives: `objs.len()=1`, `locs.len()=6`, `seqs.len()=8`, and `spots.len()=4`. These are public default config definitions owned by each test client; no shared Arc table is mutated.
- Direct and enclosed paths remain exercised. Assertions cover loc changes and expiry, loc animation edge/Southeast heights, object add/count/delete/reveal and world pile state, signed projectiles and map animations, merge attachment/scheduling, area-sound gates/effects, rebuild lifecycle, exact cursor consumption, and generation/publication effects.
- The existing `enclosed_rejects_unknown_short_and_invalid_tail_before_sound_prefix` cases retain distinct out-of-range loc/object/spotanim and malformed-frame assertions, so permissive validation cannot make positives green.

No cache directory was populated and no client/server cache artifact was added.

## Verification

All commands used `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target` and were serialized.

- Before correction, exact stage2 enclosed test failed with `invalid zone loc`; zones failed 4/8 with the table-bound assertions above.
- Three sequential isolation passes, each using fresh nonexistent cache paths:
  - stage2 enclosed regression: 1 passed, 0 failed, each pass.
  - cleanup E zones: 8 passed, 0 failed, each pass.
- Full stage2 suite: 48 passed, 0 failed.
- Full cleanup E zones suite: 8 passed, 0 failed.
- Full native workspace all-features serialized command (`cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`): Cargo target summaries are all zero failures, including client lib 76, GPU suites, stage1 46, stage2 48, stage3 23, zones 8, legacy zone 45, client-play 4 and doc tests. The preserved tee wrapper log `target/offline-fixture-isolation-workspace-final.log` ends with Cargo summaries passing but wrapper exit 255 because its shell pipeline status extraction used an empty `pipestatus` value; this wrapper result is retained as uncertainty, not counted as a test failure. A direct non-pipelined rerun of the same Cargo command returned `workspace_cargo_exit=0`.
- Contract verifier: `PASS: 256 inbound, 82 outbound rows; 50 fixtures`.
- `git diff --check`: pass.
- No active cargo/test process remained after verification.

## Remaining gates

This closes only offline fixture isolation. Root still owns H authorization, authentic 289 cache/server pairing, live RSA/ISAAC/login/action/presentation proof, and required final Grok4.6 whole-branch review. Synthetic config definitions and offline pixel/request assertions do not establish live or audible presentation compatibility.
