# Fairy Ring — Client-Rust

Native **RuneScape revision 274** (~2004) client: a structural 1:1 Rust port of Lost City `Client-TS` 274.

| | |
|--|--|
| **Public brand** | **Fairy Ring** |
| **Branch** | `rs2-r274` |
| **Upstream lineage** | [LostCityRS/Client-TS](https://github.com/LostCityRS/Client-TS) 274 (`webclient`) + [Client-Java](https://github.com/LostCityRS/Client-Java) for TCP/`signlink` |
| **Companion** | [Fairy Ring workspace](https://github.com/Fairy-Ring/fairy-ring-workspace) |

Derived from open Lost City work under MIT. **Not** official Lost City / LostCityRS and **not** Jagex.

This tree is a **playable/headless client library** (`crates/client`) plus `client-play`. It is not a TypeScript bot. A later bot adapter may read Java-public fields; this crate does not grow a bot API.

## Run (local LC engine)

The local engine must listen on `127.0.0.1:43594` (game) and `:80` (`/crc`).

```bash
./tools/redeploy.sh          # bake RSA from $HOME/experiments/Server/engine/data/config/private.pem
./target/debug/client-play --window
```

`--window` is the 765×503 applet (title is the control plane). Add `--user`/`--pass` to skip title login. Omit `--window` for headless. `window`/`audio` are default features of `client-play`, so a later `cargo build -p client-play` still has the present backend.

First login auto-registers. Expected: `ingame`, then `tile: <x> <z> (cycle N)`.

`cargo run` without `LOGIN_RSAN`/`LOGIN_RSAE` rebakes the Java default key. Use `tools/redeploy.sh` when the engine pem is not that key. Login jag CRCs come from `GET /crc` (same as client-ts).

## License

MIT (Lost City). See `LICENSE`.
