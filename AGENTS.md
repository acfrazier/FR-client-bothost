# Agent rules — FR-client-rust (rs2-r274)

Read this file once. Do **not** search the disk for another `AGENTS.md`.

**What this is:** Fairy Ring 274 Rust client. Worktree: `.worktrees/feat-274-client-port`. Push `fr/rs2-r274`, no PR. Not a bot repo.

**Do not:** add a bot API; add a dummy tick-end opcode; put bot crates here; invent packets.

**Already landed:** `Client.cache: Arc<Cache>` (no live ifaces); `Client.ifaces`; `Client.gens` / `bump_gens`; `logout()` bumps all gens; per-client `login_uid` (not `1337`); `Client::from_shared`. Customizations: `docs/client-customizations.md`.

**Do:** TDD as the task brief. `cargo test -p client --offline`. One task only. Commit on `feat/274-client-port`. Write the report file the orch named.

**Live client play:** `./tools/redeploy.sh` then `./target/debug/client-play --window`. Account `test`/`test`.
