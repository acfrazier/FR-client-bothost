# Session-profile client final receipts

Compiler cache for every Cargo command:

`CARGO_TARGET_DIR=/Users/acfrazier/experiments/274bot/.worktrees/rs2b0t-multirevision/target/client`

## Focused affected suites

Command:

`cargo test -p client --test session_profile --test from_shared --test client_stream --test login --test ondemand --test maininit -- --test-threads=1`

Exit: 0.

Results:

- client_stream: 2 passed
- from_shared: 7 passed
- login: 10 passed
- maininit: 14 passed
- ondemand: 16 passed
- session_profile: 9 passed

## Parallel regression for the new suite

Command run twice:

`cargo test -p client --test session_profile`

Both exits: 0. Each run: 9 passed, 0 failed.

## Full client crate

Command:

`cargo test -p client --all-targets`

Exit: 0. Every executed Cargo target summary reported zero failures. The existing GPU-adapter-only unit test remained ignored; no GPU or live-game result is claimed.

## Format and strict Clippy

Commands:

- `cargo fmt --all -- --check`
- `cargo clippy -p client --all-targets -- -D warnings`
- `git diff --check`

All exits: 0. Strict Clippy completed with no warnings after grouping private constructor state into `ClientConstruction`.
