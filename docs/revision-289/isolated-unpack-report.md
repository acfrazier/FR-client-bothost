# Isolated standalone unpack output

## Scope

The standalone 289 client previously used `HOME/.274bot/unpack` for production
JAG downloads and versioned snapshots. That location is shared with existing
274 work, even when the production cache pack path is selected.

## Change

`crates/client/src/transport.rs` honors the explicit
`CLIENT_UNPACK_DIR` environment variable when it is non-empty. An absent or
empty override preserves the standalone client's prior fallback:

- `$HOME/.274bot/unpack` when `HOME` is non-empty
- `.274bot/unpack` otherwise

This override supplies the unpack directory only for an unbound standalone
client. Bound host sessions receive both paths from `ClientSessionProfile`;
274bot launch profiles default cache and unpack together to
`~/.274bot/unpack` for revision 274 or `~/.274bot/unpack-289` for revision 289,
regardless of transport. Standalone callers reach the fallback through
`unpack_dir()`.

## Verification

Client tests cover explicit transport selection and the standalone path helpers
through cache/session consumers:

    cargo test -p client --all-targets

The parent orchestrator must set `CLIENT_UNPACK_DIR` to its authorized runtime
location before any standalone live verification. This change does not claim
live cache pairing, HTTP constructor compatibility, server login, or client
acceptance.
