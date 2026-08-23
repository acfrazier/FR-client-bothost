# Agent rules — FR-client-bothost (r274-bothost)

Read this file once. Do **not** search the disk for another `AGENTS.md`.

**What this is:** 274bot host client fork. GitHub: `acfrazier/FR-client-bothost` `r274-bothost`. Fairy-Ring `rs2-r274` is the unmodified 274 client — **do not push there**.

**Do not:** add a bot action API; add a dummy tick-end opcode; put 274bot crates here; invent packets; change packet timing / ISAAC / `doAction` / `tryMove` behavior.

**Already landed:** `Client.cache: Arc<Cache>` (no live ifaces); `Client.ifaces`; `Client.gens` / `bump_gens`; `logout()` bumps all gens; per-client `login_uid` (not `1337`); `Client::from_shared`; skip-paint (`draw` / `set_draw`); stream `bytes_in`/`bytes_out`; `game_draw_enters` / `title_screen_draw_enters`; host-stamped `loop_ns`/`raster_ns`/`paint_n`/`skip_n`. Customizations: `docs/client-customizations.md`. This fork **is** the hook tree; Fairy-Ring is not.

**Do:** TDD as the task brief. `cargo test -p client --offline`. One task only. Commit on `r274-bothost`. Write the report file the orch named.

**Live client play:** `./tools/redeploy.sh` then `./target/debug/client-play --window`. Account `test`/`test`.
