# client-play

Thin CLI that logs into a local 274 engine over TCP and runs the 20 ms
client machine. RSA is baked at compile time; run the binary
`tools/redeploy.sh` produces, not a later `cargo run` (see below).

## Setup

1. Start the local Server (the 274 engine) so it listens on
   `127.0.0.1:43594`.
2. From this repo, rebuild with the engine's key:

   ```bash
   ./tools/redeploy.sh
   ```

   This extracts `$HOME/experiments/Server/engine/data/config/private.pem`
   and builds `client-play` with the matching public half baked in.
3. Run the built binary:

   ```bash
   ./target/debug/client-play --user <u> --pass <p>
   ```

   Options (defaults): `--host` `127.0.0.1`, `--port` `43594`, `--cache`
   `$HOME/experiments/Server/engine/data/pack/client`. `--window` opens the
   789×532 applet (redeploy builds the `window,audio` features in) and opens
   the cpal speaker; a window open failure exits 1, an audio device failure
   logs and continues without sound. Omit `--window` for headless — the same
   binary, no window and no audio. Nothing here is required to log in.

## Expected output (live proof, operator step — needs the Server running)

- **Wrong key** (skip redeploy against a rotated engine pem): login code 6,
  "Wrong RSA key - run tools/redeploy.sh and rebuild". Exit non-zero; there
  is no title-screen retry loop.
- **After `./tools/redeploy.sh`**: login code 2, `ingame`, then the world
  rebuilds (`REBUILD_NORMAL` → `scene_state`) and the local-player tile
  prints every 50 `loop_cycle`s once player info is wired:
  `tile: <x> <z> (cycle N)`.

With the Server down, the CLI exits with the connect error and the unit
suite still covers `lostCon` (`cargo test -p client`).

## Why not `cargo run`

`build.rs` bakes `LOGIN_RSAN` / `LOGIN_RSAE` from the environment. Any later
`cargo build` / `cargo run` without those env vars rebakes the Java default
key — wrong for a freshly generated engine pem. `tools/redeploy.sh` builds
with the right env, so run the artifact it produced. You only need to
redeploy again when the engine key changes.
