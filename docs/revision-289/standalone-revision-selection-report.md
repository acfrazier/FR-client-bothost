# Standalone revision selection

Implemented on branch `codex/revision-289-client` from `fc5516c`.

## Change

`crates/client-play/src/main.rs` now accepts `--revision 274|289` as a space-separated value. Revision 274 remains the default, and the selected `ClientRevision` is passed directly to `Client::new_with_revision` before renderer, window, audio, cache loading, or network startup. Invalid or missing revision values return through the existing usage/exit-2 path.

The parser is split into a testable iterator seam while preserving the existing CLI options and behavior.

## Verification

With `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`:

- `cargo test -p client-play --no-default-features` — 4 passed.
  - default revision is R274
  - explicit 289 selection preserves other options
  - invalid and missing values are rejected
  - selected revision is observed through the existing `Client::from_shared_with_revision` construction seam without live services
- `cargo check -p client-play` — passed.
- Built headless binary and ran `target/debug/client-play --revision 290` — exit 2 with usage, before client initialization.
- `git diff --check` — passed.

The repository has pre-existing dirty files outside this bounded task; they were not modified. Full `cargo fmt --all -- --check` remains blocked by pre-existing formatting differences in client sources; the changed `client-play` file was formatted directly with `rustfmt`.

No live server, login, cache, rendering, or whole-client acceptance is claimed.
