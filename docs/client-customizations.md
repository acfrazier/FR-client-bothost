# Client customizations for bot-host plumbing

Campaign-1 additions to the `crates/client` port, consumed by the bot host. The
crate stays a faithful 274 client: no bot API, no snapshot/query surface beyond
the public fields below.

## Shared cache (`Arc<Cache>`)

`Client.cache` is an `Arc<Cache>`: the config type tables (`obj`, `npc`, `loc`,
...) are unpacked once and shared read-only across every `Client` in a process.
The per-client mutable state lives in `Client.ifaces`. (Task 1)

## Packet-family generations (`ClientGens`)

`Client.gens` (`ClientGens { npc, player, inv, varp, stat, chat, scene }`)
bumps a `u64` counter after every applied packet via
`Client.bump_gens(ServerProt)`. The host polls the counters to tell which world
slices changed since its last read. `handle_packet` bumps after every
successful dispatch; `logout()` (T1, T2, `LOGOUT`, `lost_con`) bumps every
family so wipe paths do not leave a live snapshot.
(Task 2)

## Per-client login uid

`Client.login_uid: i32` is random per `Client` (clock XOR `AtomicU64`, never `0`
or the old shared `1337`) and is written into the 274 handshake RSA block at
`login()`. The host may overwrite it (e.g. from a profile uid) before `login`.
(Task 3)

## Shared construct

`Client::from_shared(config, Arc<Cache>, ifaces)` skips `load_cache` and the
`/crc` probe so the host can unpack once per `cache_dir`. `Client::new` still
unpacks for tests/`client-play`. `error_loading` is false after a successful
`from_shared`.

## No bot API

There is deliberately no bot-facing action API; the host drives the client
through the public fields above and the existing `login` / packet surfaces.
