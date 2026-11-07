# FR-client-bothost (`r274-bh-modular`)

Bot-host fork of the modularized Fairy-Ring 274 client. This is the
**client** [274bot](https://github.com/acfrazier/274bot) compiles as a
path-dep (`vendor/fr-client-rust`). Alpha with the host’s `0.1.1` tag.

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
skip-paint / `set_draw`, shared cache, GPU device inject, and the
`Client::set_journal_paint_hidden` /
`Client::journal_paint_hidden` paint lease. The lease hides only the main
modal raster while retaining the pre-read side and tab chrome; simulation,
modal/tab state, and packet-producing icon edges stay live.

`headed-fixture` is an opt-in window feature for live test fixtures. On Windows
it lets one fixture worker create, pump, present, and drop its own winit event
loop. The host's `journal-paint-proof` feature enables it; panel-play and
tui-play do not. Normal event-loop initialization is unchanged.

Textured GPU lighting follows CpuPix3D's integer scanlines, eight-pixel
brightness bands, and packed texture-palette arithmetic. The scene vertex
stays 24 bytes: the vertex shader reads each textured triangle from a
read-only storage view of the same reused vertex buffer. There is one
vertex upload per rendered scene frame, with no extra stream or upload.
GPU fringe scanlines and blocks are bounded, and near-clipped polygon fans
share the CPU's horizontal-clipping decision. Transparent mips that shade
to zero preserve the background; opaque black still draws. Untextured
shading, texture filtering/UVs, and the scene-state-1 last-frame freeze are
unchanged.
GPU triangle coverage and projective texture sampling still differ from
the CPU's integer rasterizer; this is not a pixel-identical GPU backend.

The final textured shade is also clamped to its triangle's vertex range,
including CPU-covered pixels. This intentionally differs from CpuPix3D's
rare below-minimum tails: integer stride rounding can select the next
brighter band, or pure black at shade 0 (about 0.09% of CPU-covered pixels
in the review's gentle-gradient probe). The clamp prevents those rounding
undershoots from producing pure-black GPU artifacts instead of reproducing
that CPU quirk; legitimate opaque black texels still draw.

GPU self-initialization requires vertex-stage storage-buffer support;
adapters without it use the CPU fallback.

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

Revision 377 explicitly selects Java's 5000-slot sound/delay tables; the
274/289 default and shared 1000-slot tables are unchanged. Sound loading is
enabled in high-memory construction, startup, and disconnected memory-mode
changes. The real-cache regression in `revision_377_sounds` checks every
sound's occupancy, optimized delay, loop bounds and tone timings against
the original Java 377 loader, including IDs above 999. Regenerate its fixture:

```bash
python3 tools/generate_revision_377_sounds.py \
  --java-root "$JAVA_377_ROOT" --cache-dir "$CLIENT_377_CACHE_DIR" \
  --work-dir "$ORACLE_WORK_DIR"
```

`live_sound_377` is an ignored opt-in proof of profile-bound, shared-table
high-memory construction, rendered scene2 and server logout on Fairy Ring's
local 377 engine. It requires `ENGINE_DIR`, `CLIENT_377_RUNTIME_CACHE_DIR`,
`CLIENT_377_LIVE_DIR` and an explicit `CLIENT_377_LIVE_USER`.
Every 377 live fixture checks `CLIENT_377_LIVE_PREFIX` (default `t2a`).
Set it to the assigned account prefix, for example `s12f`. Empty prefixes
and accounts outside that prefix are rejected before connecting.
It proves loading and login, not audible playback or PCM parity.

Revision 377 uses separate Java-sized media tables: 100 slots each for
mapscene and mapfunction images, with 72 and 70 images loaded. Its player
PK, prayer and hint icons remain separate, and its minimap supports the
fifth (team) dot and multiway overlay. The default 274 profile and the
274/289 sprite paths keep their existing tables and behavior.
The PK and prayer indices use signed bytes. The equipment-team value and
legacy headicon bitmask use unsigned bytes, preserving the existing inline
player layout and all wire values.
The 377 runtime validates `SET_MULTIWAY` before applying its unsigned byte
to the Java multiway overlay state.

The optional mainland proof in `live_interactions_377` continues a naturally
completed tutorial account. It selects revision 377 explicitly, walks to
Lumbridge goblins, records attack styles, damage, health bars, XP, a level
and loot pickup, then walks to Draynor for Deposit All, Withdraw 1,
Deposit 1 and the no-PIN bank path. It also captures minimaps at Lumbridge,
Draynor and Varrock. No account state is seeded.
The combat and Varrock phases exercise all four weapon-style selectors and
wait for received server varp updates, not only the local button highlight.
Combat then uses Accurate for the measured Attack XP.
Travel uses rendered minimap clicks and records the production-selected
intermediate endpoints. The final Varrock landmark and received bank
inventory changes are checked exactly.

Use the same four environment variables as `live_sound_377`. Set
`CLIENT_377_STAGE11` to `combat`, `bank` or `varrock` for separate,
server-logged-out phases, or `full` for the complete route:

```bash
cargo test -p client --no-default-features --test live_interactions_377 \
  mainland_lesson_377::mainland_combat_bank_377::natural_mainland_combat_and_bank_377 \
  -- --exact --ignored --nocapture --test-threads=1
```

Lossless frames and adjacent snapshots retain the received actor poses,
hits, component mutations, inventory counts, camera and map base for
comparison with the original Java client. XP uses Java 377's stats and
level presentation, not a modern floating XP overlay. Revision 377's
inventory buttons 1–5 and item drag use Java's transformed packet bodies,
including bank 10/X amounts, shop quantities, smithing and trade. Every
Java 377 `OP*` menu packet has a revision-specific writer and dispatch;
the 274/289 layouts and default revision stay unchanged. A cold 377 login
clears the multiway overlay, while a reconnect preserves it as Java does.

Revision-377 public chat uses Java's case-insensitive colour and effect
prefixes, then writes colour with `p1_alt2` and effect with `p1_alt1`.
Its effects are `wave`, `wave2`, `shake`, `scroll` and `slide`. Private
chat uses the revision-377 variable-length packet with the recipient's
base-37 name and WordPack text. The 80 packet descriptors are in
[`protocol-377.json`](docs/revision-377/protocol-377.json).
`CLIENT_CHEAT` uses Java's UTF-16-unit length and low-byte `pjstr` encoding
for both raw `::` chat and granted API calls.
Report rules 601..=613 close interfaces and send `SEND_SNAPSHOT` (184) for a
nonempty target, with reason 0..=12 and the mute flag; action 620 only toggles
the mute option. `EVENT_MOUSE_MOVE` keeps the `Client.ts:2317` 240-byte
threshold. Java's inverted `Client.java:3044` predicate is a bug, bounded on
purpose to keep the one-byte body length safe; Rust retains Java's field
encoding and leaves excess samples queued.
`EVENT_SYNTH_ERROR` (80) reports an unresolved 377 wave as `p2(waveId &
0x7fff)` by default. `Client::set_synth_error_reporting_377(false)` selects
Java's reporting-disabled `p2(-1)` form without masking the sentinel.
`P_NAMEDIALOG` (server 6) opens Java's name prompt and renders “Enter name:”;
keys 32..=122 append up to 12 characters, backspace deletes, and nonempty
Enter emits `RESUME_P_NAMEDIALOG` (206) with `p8(toBase37(name))`. Empty
Enter sends no response. `P_COUNTDIALOG` and `IF_CLOSE` clear name mode, so
the amount prompt cannot inherit the name-response opcode. Rust has no writer
for the Java AWT-only `EVENT_HAS_WINDOW`.
The bank X actions accept Java's bodyless `P_COUNTDIALOG` (58), close any
social input, clear and open the amount prompt, and redraw chat only after
the complete frame has been validated.

`live_bank_shop_377` is an ignored proof for an explicitly selected prepared
account near Draynor bank. It checks Withdraw/Deposit-10 and X, amount
prompts, an inventory drag, and Buy/Sell-5 against received server state.
It walks through the production pathfinder to Lumbridge General Store,
buys and sells five hammers from its real stock, then returns to Draynor.
It requires the same live engine/cache variables and explicit JSON labels
in `CLIENT_377_BANK_SHOP_INTERVENTIONS` (`[]` when none).
`CLIENT_377_BANK_SHOP_PREPARE=1` optionally gives only that account enough
coins and buckets before the tested actions. The proof records each
intervention. The account must already hold at least 10 banked coins.

`live_production_377` is an ignored CPU-rendered proof of the production
interfaces offered by Fairy Ring's selected 377 content. It exercises a
bronze furnace smelt, Varrock's anvil inventory and dagger production,
an ungated Lumbridge range's Cook X and Cook All choices, a spinning wheel, and a
product-earned level dialog and firework. Each phase records received
inventories, XP, interfaces, outbound packets, fixture interventions,
and lossless full-frame and 512×334 viewport PNGs.
The knife-on-logs phase records the received server response, inventory
and XP deltas, and any product or quantity interface as an observation.
It does not assume a missing or working choice, manufacture a product
or Make-X interface, or make a historical claim about that quantity choice.
Revision 377 validates `LOC_ANIM` (142), including every enclosed child,
before applying its sequence to an existing scene slot. It retains Java's
four corner heights, wall/decoration shapes, angles, and strict edge bounds.
A wall animation with packet shape other than 2 replaces only the first
model. The second model keeps its static placement or earlier animation,
including when multiple packets arrive before the first rendered frame.
The 274/289 animation paths remain unchanged.

The 377 local Close action unloads the side, chat and main layers, restoring
decoded component defaults while retaining inventory contents. As Java 377
does, only an open side or chat layer clears the Continue debounce.
`live_interactions_377::local_close_unloads_guide_text_and_resets_continue_377`
checks this on a fresh account with real Guide dialogue and no state seeds.
Run that ignored test by its exact name to avoid the longer tutorial proof.

Run only against an already-running Fairy Ring engine and an explicit
account that has completed the tutorial. Set `ENGINE_DIR`,
`CLIENT_377_RUNTIME_CACHE_DIR`, `CLIENT_377_LIVE_DIR`,
`CLIENT_377_LIVE_USER`, `CLIENT_377_LIVE_PASSWORD`, and
`CLIENT_377_QUIET_LOCK`, then run:

```bash
cargo test -p client --no-default-features --test live_production_377 -- --ignored --nocapture
```

`CLIENT_377_PRODUCTION_PHASE` selects `all` (default), `furnace`,
`anvil`, `cook`, `fletch`, `craft`, or `levelup`.
`CLIENT_377_PRODUCTION_PREPARE=1` admits and records missing raw inputs
and, only when necessary, a Smithing level-one baseline reset; the
production outputs and level increase are always earned by real actions.
Without that opt-in, prepare the inputs and level-one account yourself.
XP assertions respect the server's whole-XP wire truncation.
For a repeat run with a full inventory, `CLIENT_377_PRODUCTION_DROP_BARS=1`
together with `PREPARE=1` separately permits real inventory Drop actions
on prior bronze-bar outputs, only as needed to make room for the next ores.
Every drop and its received inventory change is recorded as an intervention;
without this explicit opt-in, the fixture does not discard outputs.

`gpu_fountain` includes self-contained Metal/wgpu readback regressions for
textured scanline lighting, subpixel edges, and near-clipped polygons.
Its additional actual-model differential is opt-in and needs a versioned
274 cache snapshot containing `models.bin`, `config`,
and `textures`, not a running server:

```bash
FOUNTAIN_SNAPSHOT=/absolute/path/to/snapshot \
FOUNTAIN_OUTPUT=/absolute/path/to/output \
cargo test -p client --test gpu_fountain offline_fountain_views -- --ignored --exact --nocapture
```

It compares CPU/GPU yaw-0 views at pitches 128/256/383, emits 512×334 RGB
images and metrics, and isolates lighting with an opaque-white basin.

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
