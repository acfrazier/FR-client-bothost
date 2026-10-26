# FR-client-bothost (`multirev-bh-modular`)

Bot-host fork of the modularized Fairy-Ring 274 client. This is the
**client library** [274bot](https://github.com/acfrazier/274bot) compiles as a
path dependency (`vendor/fr-client-rust`).

| | |
|--|--|
| **This repo** | [acfrazier/FR-client-bothost](https://github.com/acfrazier/FR-client-bothost), default branch `multirev-bh-modular` |
| **Host** | [acfrazier/274bot](https://github.com/acfrazier/274bot) |
| **Lineage** | Modularized [Fairy-Ring/FR-client-rust](https://github.com/Fairy-Ring/FR-client-rust) client, itself derived from Lost City Client-TS / Client-Java |
| **License** | MIT ([LICENSE](LICENSE), [NOTICE.md](NOTICE.md)) |

## Revisions

Two protocol profiles, selected explicitly per session (`ClientRevision`:
`R274` default, `R289` opt-in): **289** (production) and **274**
(best-effort). The 289 inbound/outbound tables live in
[`docs/revision-289/protocol-289.json`](docs/revision-289/protocol-289.json).

## Bot-host hooks

- Per-client draw switch / skip-paint slots (`Client::set_draw`)
- Shared cache and interfaces (`Client::from_shared*`: one `Arc<Cache>` per cache dir)
- GPU device injection (`render::backend::gpu::inject_device`)
- Per-packet-family generation counters (`Client::gens`)
- Journal paint lease (`Client::set_journal_paint_hidden` / `journal_paint_hidden`)
- Nav debug paint (`Client::set_nav_debug_paint`, projected as a GPU overlay)
- Local-player animation observations (`Client::local_animation_update`)

**There is no bot action API in this crate.** Host snapshot / interact /
nav live in 274bot. Do not add one here.

## Renderers, crates, build, test

- Headed default is a **wgpu GPU** 3D renderer; `BOT_CPU=1` selects CpuPix3D.
- `crates/client` is the library 274bot links; `crates/client-play` is a thin
  applet CLI (`--window` opens the 765×503 highmem applet, headless defaults
  lowmem; `--revision 274|289` selects the protocol profile).
- Build and test: `cargo build -p client-play`, `cargo test -p client`.
  `tools/` holds fixture generators, the 289 contract generator/verifier, and
  `redeploy.sh` (reads a rotated engine `private.pem`; stock keys need no step).
- 274bot links this tree as a path dependency at `vendor/fr-client-rust`,
  pinned to the client tag of each 274bot release.

## Branches and tags

- Default branch **`multirev-bh-modular`** (renamed from `r274-bh-modular` on
  2026-10-10) carries 289 and 274, and is fast-forwarded at each 274bot release.
- Tags `274bot-<version>` (`274bot-0.1.0` … `274bot-0.2.0`,
  `274bot-0.2.0.1` once released) mark the exact client each 274bot release
  shipped with. `archive/<branch>` tags keep retired task branches.
- `r274-modular` is the earlier 274-only modular refactor this branch grew from
  (it already has the per-client draw switch and shared cache, but none of the
  later bot-host work or revision 289); `r274-bothost` is the pre-modular fork.
  Both are frozen.
- Not the Fairy-Ring upstream: do not push there, and do not present this
  tree as “Lost City Client,” “LC,” or Fairy Ring.

User-facing changes ship in 274bot — see its CHANGELOG, not this repo.

## Upstream

- Fairy Ring modular 274 client: https://github.com/Fairy-Ring/FR-client-rust
- Client-TS: https://github.com/LostCityRS/Client-TS
- Client-Java: https://github.com/LostCityRS/Client-Java
