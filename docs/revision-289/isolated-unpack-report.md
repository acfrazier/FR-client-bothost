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

The override is used only for WSS unpack/cache transport. TCP cache selection
remains `$ENGINE_DIR/data/pack/client`; `HOME` is not repurposed. Existing
callers reach the behavior through `unpack_dir()` and
`cache_dir_for(Transport::Wss)`.

## Verification

Client tests cover explicit transport selection and the standalone path helpers
through cache/session consumers:

    cargo test -p client --all-targets

The parent orchestrator must set `CLIENT_UNPACK_DIR` to its authorized runtime
location before any standalone live verification. This change does not claim
live cache pairing, HTTP constructor compatibility, server login, or client
acceptance.
