# FR-client-bothost (`r274-bh-modular`)

Bot-host fork of the modularized Fairy-Ring 274 client. This is the
**client** [274bot](https://github.com/acfrazier/274bot) compiles as a
path-dep (`vendor/fr-client-rust`). Alpha with the host’s `0.1.0` tag.

| | |
|--|--|
| **This repo** | [acfrazier/FR-client-bothost](https://github.com/acfrazier/FR-client-bothost) **`r274-bh-modular`** |
| **Host** | [acfrazier/274bot](https://github.com/acfrazier/274bot) |
| **Lineage** | Modularized [Fairy-Ring/FR-client-rust](https://github.com/Fairy-Ring/FR-client-rust) 274 client, itself a derivation of Lost City Client-TS 274 / Client-Java 274 |
| **License** | MIT ([LICENSE](LICENSE), [NOTICE.md](NOTICE.md)) |

## What it is

A Rust 274 client **library** (`crates/client`) plus a thin applet CLI
(`crates/client-play`). Headed default is a **wgpu GPU** 3D renderer;
`BOT_CPU=1` is CpuPix3D. Bot-host hooks in this fork: gen counters,
skip-paint / `set_draw`, shared cache, GPU device inject. Packet timing
and `doAction` stay Java-shaped.

**There is no bot action API in this crate.** Host snapshot / interact /
nav live in 274bot. Do not add one here.

## What it is not

- **Not** Jagex, **not** official Lost City / LostCityRS, **not** a Fairy
  Ring release. Do not present this tree as “Lost City Client,” “LC,” or
  Fairy Ring.
- **Not** `r274-modular` (same refactor, no bot-host hooks) and **not**
  `r274-bothost` (pre-modular fork — do not push there).
- Do **not** push `Fairy-Ring/FR-client-rust` or `LostCityRS/*`.

## Run (applet, local 274 engine)

The operator window for the **host** is 274bot `panel-play`, not this
CLI. `client-play --window` is the 765×503 `Present` applet for fidelity.

```bash
./tools/redeploy.sh          # bake RSA from the engine private.pem
./target/debug/client-play --window
```

`--window` is the applet (highmem). Omit it for headless (lowmem).
`--lowmem` / `--highmem` override. `--user` / `--pass` skip the title
form. Login errors stay on the title so Login can be retried.

Stock Lost City Server uses the Java default login RSA — no bake.
`$ENGINE_DIR` (default `$HOME/experiments/Server/engine`) is the cache
and optional rotated `data/config/private.pem`. Jag CRCs come from
`GET /crc`. Cache: `$ENGINE_DIR/data/pack/client`.

More CLI detail: [`crates/client-play/README.md`](crates/client-play/README.md).

## Tests

```bash
cargo test -p client
```

274bot `cargo test` does **not** run these. Live engine tests stay in
274bot (`LIVE=1 cargo test -p e2e` / `-p host-play`).

## Completeness disclaimer

We do **not** claim this tree **is** authentic, original, or complete.
Work is ongoing under an accuracy bar; humans and agents make mistakes.

## License

MIT as upstream. Do **not** relicense Lost City–originated code as
original work of this project. See [NOTICE.md](NOTICE.md).

## Upstream

- Fairy Ring modular 274 client: https://github.com/Fairy-Ring/FR-client-rust
- Client-TS: https://github.com/LostCityRS/Client-TS
- Client-Java: https://github.com/LostCityRS/Client-Java
