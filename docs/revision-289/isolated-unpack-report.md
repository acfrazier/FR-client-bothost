# Isolated standalone unpack output

## Scope

The standalone 289 client previously used `HOME/.274bot/unpack` for production
JAG downloads and versioned snapshots. That location is shared with existing
274 work, even when the production cache pack path is selected.

## Change

`crates/client/src/bot_target.rs` now honors the explicit
`CLIENT_UNPACK_DIR` environment variable when it is non-empty. An absent or
empty override preserves the exact prior fallback:

- `$HOME/.274bot/unpack` when `HOME` is non-empty
- `.274bot/unpack` otherwise

The override is used only for the production unpack/cache target. Local cache
selection remains `$ENGINE_DIR/data/pack/client`; `HOME` is not repurposed.
Existing callers reach the behavior through `unpack_dir()` and
`cache_dir_for(BotTarget::Prod)`.

## Verification

Pure helper tests cover an absolute override, absent and empty fallback, and
local-versus-production target behavior. The targeted client library test run
passed:

    CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client bot_target::tests::unpack_dir_override --lib
    2 passed; 0 failed

The parent orchestrator must set `CLIENT_UNPACK_DIR` to its authorized runtime
location before any standalone live verification. This change does not claim
live cache pairing, HTTP constructor compatibility, server login, or client
acceptance.
