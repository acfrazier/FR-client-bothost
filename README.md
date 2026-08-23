# Fairy Ring — Client-Rust

Native **RuneScape revision 274** (~2004) client: a structural **1:1 Rust port** of Lost City **Client-TS 274**, with **Client-Java 274** as the TCP / `signlink` oracle.

| | |
|--|--|
| **Public brand** | **Fairy Ring** (client-rust tree) |
| **Branch** | `rs2-r274` |
| **Upstream lineage** | [LostCityRS/Client-TS](https://github.com/LostCityRS/Client-TS) 274 (`webclient`) + [LostCityRS/Client-Java](https://github.com/LostCityRS/Client-Java) |
| **Companion workspace** | [Fairy Ring workspace](https://github.com/Fairy-Ring/fairy-ring-workspace) |

## Derived from Lost City — not Lost City

This repository is a **derivation** of open **Lost City / LostCityRS** client work. We build on those trees under their licenses.

**Derivation does not mean official.** This is **not** official Lost City / LostCityRS and is **not** endorsed by Jagex Ltd. rs2b0t/rs2b2t patterns may be used as tools; this crate is **not** their product layer and does **not** grow a bot API.

The 274bot host’s client fork (instrumentation, skip-paint, shared cache) is [`acfrazier/FR-client-bothost`](https://github.com/acfrazier/FR-client-bothost) `r274-bothost`. That fork still has no bot action API; this Fairy Ring tree stays a 274 client and does not grow one.

Do **not** present this repo as “Lost City Client,” “LC,” or official LostCityRS.  
See [NOTICE.md](NOTICE.md).

## AI use (explicit)

Development of this fork **uses AI tools and coding agents**. Humans own product judgment. **Bot / harness hooks must not be installed in this tree** — a later adapter may read Java-visible fields from a separate repo. Host-side client work lives on [`FR-client-bothost`](https://github.com/acfrazier/FR-client-bothost) `r274-bothost`.

## What this tree is

| This repo | Not this repo |
|-----------|----------------|
| Playable/headless 274 client library (`crates/client`) | TypeScript bot, orange panel, nav, scripts |
| `client-play` CLI (765×503 applet or headless) | Full bot host / multi-client wall |
| Java-faithful Pix3D painter, packets, HUD | 377 / 410 client, world map, GPU |

## Companion repos

Public under **Fairy Ring** (same brand family; separate remotes):

| Repo | Role |
|------|------|
| [FR-engine](https://github.com/Fairy-Ring/FR-engine) | 377 engine (not this client’s local 274 world) |
| [FR-client-ts](https://github.com/Fairy-Ring/FR-client-ts) | Browser client (377) |
| [FR-content](https://github.com/Fairy-Ring/FR-content) | Period content (377) |
| [fairy-ring-workspace](https://github.com/Fairy-Ring/fairy-ring-workspace) | Docs, harness toys, residual bar |

A **local 274 engine** (Lost City Server layout: game `43594`, HTTP `/crc` on `:80`) is what `client-play` talks to today.

## Run (local 274 engine)

```bash
./tools/redeploy.sh          # bake RSA from the engine private.pem
./target/debug/client-play --window
```

`--window` is the 765×503 applet (highmem). Omit it for headless (lowmem). `--lowmem` / `--highmem` override. `--user` / `--pass` skip the title form (not `maininit`). Login errors stay on the title screen so Login can be retried.

`cargo run` without `LOGIN_RSAN` / `LOGIN_RSAE` rebakes the Java default key. Use the binary `tools/redeploy.sh` produced. Jag CRCs come from `GET /crc`.

More CLI detail: [`crates/client-play/README.md`](crates/client-play/README.md). Product docs: [`docs/README.md`](docs/README.md).

## Completeness disclaimer

We do **not** claim this tree **is** authentic, original, or complete. Work is ongoing under an accuracy bar; humans and agents make mistakes.

## License

This project is licensed under the [MIT License](LICENSE) as upstream.  
Do **not** relicense Lost City–originated code as original work of this project.  
See [NOTICE.md](NOTICE.md).

## Upstream

- Client-TS: https://github.com/LostCityRS/Client-TS  
- Client-Java: https://github.com/LostCityRS/Client-Java  
- Lost City forum: https://lostcity.rs/

**Never push experiment work to `LostCityRS/*` without explicit permission.**  
Push remote (operator): `fr` → [Fairy-Ring/FR-client-rust](https://github.com/Fairy-Ring/FR-client-rust) `rs2-r274`.
