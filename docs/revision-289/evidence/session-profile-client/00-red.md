# Session-profile client red receipts

All Cargo commands used:

`CARGO_TARGET_DIR=/Users/acfrazier/experiments/274bot/.worktrees/rs2b0t-multirevision/target/client`

1. Before the public API existed:

   `cargo test -p client --test session_profile --no-run`

   Exit: 101. The session transcript retained only the exit and not the compiler diagnostic; this is recorded as incomplete evidence rather than reconstructed output.

2. During fixture completion:

   `cargo test -p client --test session_profile -- --test-threads=1 --nocapture`

   Exit: 101. Result: 8 passed, 1 failed. `bound_maininit_uses_asset_endpoint_cache_and_expected_crc` reached `OnDemand::get_model_use` with an empty synthetic `model_index` and panicked at `crates/client/src/io/ondemand.rs:594`. The fixture was corrected to include a one-byte model index; production behavior was not weakened.

3. First default-parallel all-target run:

   `cargo test -p client --all-targets`

   Exit: 101. The session-profile suite had 8 passed and 1 failed: `bound_crc_mismatch_fails_without_replacing_frozen_identity` encountered `OnDemand identity mismatch for occupied endpoint`. Two independent tests had reused hard-coded game port 44594 while running concurrently. Both now reserve distinct ephemeral listeners. Two consecutive default-parallel focused runs and the subsequent all-target run passed.
