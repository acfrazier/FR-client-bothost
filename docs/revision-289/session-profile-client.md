# Client session-profile binding

Task: `t_1b93f768`
Branch: `codex/bothost-274-289`
Accepted base: `58120f28ee5208553ca07f41cb364f2cf98ea280`
Implementation commit: `c8e61557bf5b208eb4ffb589fd0a797fbf7d42ab`

## Change

The client crate now exposes the accepted additive `ClientSessionConfig` and immutable `ClientSessionProfile` API. Explicit profiles validate endpoints, roots, content identity, and decimal RSA values once; invalid explicit RSA fails closed. `Client::from_shared_with_profile` rejects redundant `ClientConfig` mismatches before update-worker creation and preserves the shared cache/interface Arcs.

Bound clients now use the frozen profile for login and reconnect revision, transport, endpoint and RSA; CRC and Jag HTTP bootstrap; cache, unpack, JagFX, media and texture roots; and OnDemand transport. Explicit HTTPS and WSS helpers honor their supplied ports while standalone wrappers retain ambient target selection and port 443 behavior.

OnDemand hub reuse now requires matching bound target, revision, cache root, declared content identity, and SHA-256 of actual version/CRC tables. Bound/legacy or mismatched-bound clients are rejected before joining an occupied endpoint. Matching profiles retain one shared worker, subscriber accounting, and synchronous last-subscriber shutdown. Socket adoption rejects mismatched or bound/legacy profiles before taking either stream or changing state; legacy-to-legacy behavior is unchanged.

Nine real socket/HTTP integration tests cover profile validation, constructor pre-effect rejection, frozen login/reconnect revision and RSA, bound asset bootstrap and CRC mismatch, adoption preservation, OnDemand declared/actual mismatch rejection, sharing, and last-subscriber shutdown.

## Verification

All Cargo commands used:

`CARGO_TARGET_DIR=/Users/acfrazier/experiments/274bot/.worktrees/rs2b0t-multirevision/target/client`

Passed:

- `cargo fmt --all -- --check`
- `cargo test -p client --test session_profile --test from_shared --test client_stream --test login --test ondemand --test maininit -- --test-threads=1`
  - session_profile 9, from_shared 7, client_stream 2, login 10, ondemand 16, maininit 14; zero failures.
- `cargo test -p client --test session_profile` twice with default parallel scheduling
  - 9 passed, 0 failed on each run.
- `cargo test -p client --all-targets`
  - all executed target summaries had zero failures; the existing explicit GPU-adapter test remained ignored.
- `cargo clippy -p client --all-targets -- -D warnings`
- `git diff --check`

Red and final receipts are preserved in:

- `docs/revision-289/evidence/session-profile-client/00-red.md`
- `docs/revision-289/evidence/session-profile-client/10-final.md`

## Limits

This is client-only offline and local-socket evidence. No live login, gameplay, visual, audio, or performance claim is made. The root task owns host integration, the vendor gitlink, coherent workspace gates, and any later Mac 289 engine startup checks.
