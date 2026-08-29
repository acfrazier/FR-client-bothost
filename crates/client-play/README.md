# client-play

Thin CLI that logs into a local 274 engine over TCP and runs the 20 ms
client machine. Stock Lost City RSA is the Java default (usual local-dev).
Rotated engine keys are read at runtime from `$ENGINE_DIR`.

## Setup

1. Start the local Server (the 274 engine) so it listens on
   `127.0.0.1:43594`.
2. Set `ENGINE_DIR` if the engine is not at `$HOME/experiments/Server/engine`.
   Stock LC keys need no extra step. `tools/redeploy.sh` only prints the
   public half if you rotated `private.pem`.
3. Run:

   ```bash
   ./target/debug/client-play --user <u> --pass <p>
   ```

   Options (defaults): `--host` `127.0.0.1`, `--port` `43594`, `--cache`
   `$ENGINE_DIR/data/pack/client`. `--window` opens the
   765×503 `Present` applet (highmem textures) and the cpal speaker
   (`window`/`audio` are default features). That applet is **not** the
   274bot operator window (`panel-play` in acfrazier/274bot). Headless
   defaults **lowmem** (bot host). Override with `--lowmem` / `--highmem`;
   the bot host can also set `ClientConfig.lowmem` directly. A window open
   failure exits 1; an audio device failure logs and continues without
   sound. Omit `--window` for headless. A later `cargo build` without
   `--no-default-features` will not strip the window. Nothing here is
   required to log in.

   The client **library** (what 274bot links) draws with wgpu GPU 3D by
   default; `BOT_CPU=1` is CpuPix3D. `client-play --window` stays the
   Java-shaped `Present` applet for fidelity.

## Expected output (live proof, operator step — needs the Server running)

- **Wrong key** (engine pem rotated but not visible as
  `$ENGINE_DIR/data/config/private.pem`): login code 6. The process stays
  on the title form so Login can be retried.
- **Stock LC keys**: login code 2, `ingame`, then the world rebuilds and
  the local-player tile prints every 50 `loop_cycle`s:
  `tile: <x> <z> (cycle N)`.

With the Server down, a connect error is a login error (not a process
exit); the unit suite still covers `lostCon` (`cargo test -p client`).

## `cargo run`

Fine for local-dev. RSA is chosen at login (Java default, else pem /
`LOGIN_RSAN`). There is no compile-time key bake.
