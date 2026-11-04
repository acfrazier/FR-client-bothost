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

## Explicit revision 377

Revision 274 remains the default. `--revision 289` retains the existing 289
profile. `--revision 377` selects separate 377 cache decoders and validated
Fairy Ring login, terrain movement, tutorial dialogue, door, inventory-tab,
woodcutting, and progress operations. This is not a complete 377 gameplay client.

Use revision 377 only with Fairy Ring's local 377 engine. The orchestrator
starts and stops that engine. Set `ENGINE_DIR` to that engine so runtime RSA
key selection cannot use a different local engine. Supply a cache snapshot
whose eight JAG CRCs match the engine's `/crc` response:

```bash
ENGINE_DIR=/path/to/fairy-ring-377-engine \
  ./target/debug/client-play --revision 377 --host 127.0.0.1 \
  --port 43595 --http-port 81 --cache /path/to/matched-377-cache \
  --user t2a_example --pass 'test-password' --window --cpu
```

The ignored `live_login_377` and `live_interactions_377` proofs require an
explicit, absolute Fairy Ring worktree `ENGINE_DIR` and its
`data/config/private.pem`, with no RSA overrides or fallback key. They also
require `CLIENT_377_LIVE_DIR`, `CLIENT_377_RUNTIME_CACHE_DIR`, and a
`CLIENT_377_LIVE_USER` name that starts with `t2a`. Both record the selected
engine and key path in `live-config.json`; endpoints are fixed to localhost
ports 81 and 43595. The login proof saves two scene-2 frames around a real
server logout and relog. The interaction proof uses a rendered terrain click,
accepts the initial design, captures the Guide's in-scene hint arrow, and
continues through the skip choice and five welcome pages. It then opens the
door, talks to the Survival Guide, clicks the requested backpack tab, and
chops the indicated tree twice. It records screenshots and
`rust-interactions.json` before a server-acknowledged logout.

With the orchestrator's engine already running:

```bash
ENGINE_DIR=/path/to/fairy-ring-377-engine \
CLIENT_377_RUNTIME_CACHE_DIR=/path/to/matched-377-cache \
CLIENT_377_LIVE_DIR=/path/to/evidence \
CLIENT_377_LIVE_USER=t2a_example \
  cargo test -p client --no-default-features --test live_login_377 -- \
  --ignored --nocapture --test-threads=1
```

For the interaction proof, select `--test live_interactions_377` in the same
command and use a fresh `t2a` account. Set `CLIENT_377_QUIET_LOCK` to the
operator's quiet-lock path when the environment requires it.

The `% Done` bar reads transmitted varp 406. The server-only tutorial stage
is not transmitted in varp 281. The first Guide conversation reaches stage 4,
which still has zero filled segments (`1 + floor(4 * 20 / 670) = 1`).
The first successful chop reaches stage 40. The next successful chop refreshes
the overlay to 2, which fills the first segment. The proof checks a real
server/applied varp transition from 1 to 2 and evaluates all 20 segment scripts.
No tutorial state or inventory is seeded. Screenshots show both bar states.

The 377 renderer uses Java's separate hint sprites. A literal model-and-frame
regression checks the tutorial banner against Client-Java 377. The current
pack's banner also overlaps the progress bar in Java; the renderer preserves
that geometry rather than applying a client-only layout correction.
Chat-interface replacement restores decoded non-inventory component state.
Inventory components retain their contents, as in Java's component unload.

Ordinary 377 mouse, camera, focus, idle, and Java anticheat packets use their
own encoders and trigger points. Shared writers are admitted only when their
bodies are compatible. A still-unsupported gameplay writer logs its legacy
opcode, discards the complete pending batch, and closes the session before
flushing; it does not panic or leak a 274 packet onto the 377 connection.

The ignored proof uses password `t2apass` for its `t2a` account and explicitly
enables drawing; ordinary headless clients keep the CPU-save draw gate off.

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
