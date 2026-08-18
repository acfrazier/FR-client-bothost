# 274 native client port

**Date:** 2026-08-18  
**Status:** draft (written spec; implementation plan after review)  
**Scope:** spec 1 of several — severable 274 client crate only

## Intent

Port the Lost City 274 `client-ts` to Rust as a standalone library that can log into LC-like 274 servers, run the original client machine, and later host a bot *without* the bot living inside this crate.

This is not a TypeScript client, not a TypeScript bot, and not a packet forger.

Later specs (not written here): `bot-api`, `bot-runtime`, nav, host/supervisor, scripts.

## Locked decisions

| Decision | Choice |
|---|---|
| Language | Rust |
| Simulation | Full 274 client; render optional |
| Scripts (later) | Rust crates against a sealed `bot-api`; not in this spec |
| This spec | `crates/client` + `crates/client-play` only |
| Port style | Structural 1:1 of client-ts; Java when TS is browser-only |
| I/O | Blocking TCP (`javaclient` `ClientStream`); WebSocket out of spec |
| Statics | Immutable tables process-wide; mutable client state per `Client` |
| Public surface | Java-visible fields/methods; no designed snapshot/query API |
| Music | 274 Java control plane; no tinymidipcm; no Spessa |
| Softsynth | `rustysynth` behind `audio`; headless `NullMidi` |
| RSA | Compile-time `LOGIN_RSAN` / `LOGIN_RSAE` (Java statics / TS env inline) |
| Local Server | `tools/redeploy.sh` extracts engine `private.pem` and rebuilds |

## Approach

**A, with the music and RSA amendments:** 1:1 types and cycle, Java TCP, CPU `PixMap`, process-safe statics, 274 signlink midi knobs, rustysynth optional, keys baked at compile.

Rejected: tokio/WebSocket-first (B); one-client-per-process global statics (C).

## 1. Layout and process model

```
274bot/                         cargo workspace
  crates/client/                severable 274 library (this spec)
  crates/client-play/           thin binary: log into local Server
  tools/redeploy.sh             pem → LOGIN_RSA* env → cargo build
  crates/bot-api/               later (name only)
  crates/bot-runtime/           later
  crates/nav/                   later
  crates/host/                  later
```

`crates/client` modules match client-ts:

`client`, `config`, `dash3d`, `datastruct`, `graphics`, `io`, `sound`, `util`, `wordfilter`, `mapview`

No `3rdparty/tinymidipcm`.

**One `Client` = one OS thread** running `GameShell.run`. That client also owns the original workers:

- OnDemand thread (archive 2 midi, maps, models)
- `ClientStream` writer thread (Java `ClientStream` is `Runnable`)

`Client` is `Send`. This spec never shares one `Client` across threads. A later host may spawn N clients as N threads.

### Statics

| Kind | Where |
|---|---|
| Immutable tables (trig, CRC32, skill names) | process-wide `OnceLock` |
| Packet pool, loaded `*Type`s, Pix3D draw state, JagFX tables, `loopCycle` | on that `Client` (Java `Client.loopCycle` is static; per-instance here so N clients can coexist) |
| Cache files on disk | read-only; many clients may map the same jag/idx |

### Features

| Feature | Default | Role |
|---|---|---|
| (none) | on | headless full sim |
| `window` | off | present CPU `PixMap` |
| `audio` | off | rustysynth + 274 midi hooks |

Neither feature is required to login or call `doAction` / `tryMove`.

### Port sources (in order)

1. `~/experiments/Server/webclient/src` — structure and names
2. `~/experiments/Server/javaclient` — when TS is async/browser-only (`ClientStream`, `signlink` midi, `lostCon`)
3. `~/experiments/Server/engine/src/network` — decode/encode oracle for tests

## 2. Crate surface

The library is the 274 machine. It does **not** grow `inventory()`, snapshots, or query helpers. rs2b0t measured the live client by naming fields the client already had (`RawClient` / `ClientAdapter`). Same model here: visibility follows the Java deob (`pub` where Java is `public`). A later adapter crate may name those fields. Bot nouns stay in a later `bot-api`.

**Supported construct/run path** (so `client-play` is not a scavenger hunt):

- `ClientConfig`: host, port, cache dir, members/lowmem, feature-related flags — **not** RSA
- `Client::new` + `Client::run` on the calling thread
- existing title/login methods (`login(user, pass, reconnect)`)
- `LoginError` carrying the numeric handshake code and the two `loginMes` lines

Driving the game from outside this crate is: write the menu arrays / call `doAction` / `tryMove` / read Java-public fields. Method names stay Java/TS (`doAction`, `tryMove`) so a later adapter maps onto `RawClient` without a rename layer. There is no packet-forge API.

`client-play` CLI: `--host --port --user --pass --cache [--window] [--audio]`. It logs in and runs the loop. It may print `ingame` and the local-player tile for proof. It is not a proto-adapter.

`Packet::rsaenc` still takes modulus and exponent as arguments (same as TS/Java). Cold login uses the compile-time `LOGIN_RSAN` / `LOGIN_RSAE` constants, as Java does.

## 3. Data flow

```
tools/redeploy.sh
    │  openssl from ENGINE_DIR/data/config/private.pem
    │  export LOGIN_RSAN LOGIN_RSAE
    │  cargo build -p client-play
    ▼
client-play
    │  ClientConfig (host, port, cache, members/lowmem)
    ▼
Client::new
    │  load jag/idx from cache dir (same files Server/engine packs)
    │  OnDemand worker starts
    ▼
GameShell::run                    // this thread, deltime = 20
    │  maininit → title / login
    │  login(user, pass, reconnect=false)   // opcode 16, baked RSA
    │     fail → LoginError
    │     ok   → ingame, wait REBUILD_NORMAL
    ▼
loop
    mainloop   → read TCP, decode ServerProt, update scene/ifaces/stats
                 loopCycle += 1
    mainredraw → Pix* into CPU PixMap (present only if `window`)
```

### Two clocks

The client does **not** run on the 600 ms server tick.

| Clock | What it is | Who owns it |
|---|---|---|
| GameShell `deltime = 20` | ~50 Hz `run` loop: `mainloop` then `mainredraw` | client |
| `Client.loopCycle` | increments once per `mainloop` (~20 ms when keeping up) | client |
| Server `TICKRATE = 600` | world tick; `PLAYER_INFO` / `NPC_INFO` / inv / varp land here | engine |

The 600 ms tick is **visible** to the client (inbound packets, reboot timer, hitmark/spotanim times as `loopCycle + n`). The client still walks, animates, and draws on the 20 ms loop between those packets. `setFramerate(1)` is only the error screen.

`combatCycle = loopCycle + 400` is ~8 s of *client* cycles, not 400 server ticks.

A later bot scheduler may wait on either clock. This spec implements only the 20 ms machine. Sleep and catch-up follow `GameShell.run` (ratio/count). We do not add a 600 ms client tick.

### Login

Matches 274 Java/TS: username/password → RSA blob with baked `LOGIN_RSAN`/`LOGIN_RSAE` → Isaac in/out seeds → response opcode.

- Cold login wrapper opcode **16**
- In-game reestablish (`lostCon` → `login(..., reconnect=true)`) wrapper opcode **18**
- Login response `1`: sleep 2 s, same attempt (original wait/retry)
- Login response `2`: `ingame = true`

### In-game inbound

`ClientStream` → `Packet` + Isaac → the existing `ptype` switch (`PLAYER_INFO`, `NPC_INFO`, `UPDATE_INV_*`, `VARP_*`, `IF_*`, `MIDI_SONG` / `MIDI_JINGLE`, `SYNTH_SOUND`, `REBUILD_NORMAL`, …). No parallel decoder.

### Outbound

Only what `doAction` / `tryMove` / idle / focus / camera already write. No raw opcode helper.

### Cache / OnDemand

Maps, models, midi archive 2 — same request/complete path as Java. Midi bytes go to the `Midi` backend (`NullMidi` or rustysynth). Missing jag/idx or a failed map request is a hard load error, not an empty world.

## 4. Music

**Control plane** is 1:1 from 274 Java, not 377:

- `saveMidi(fading, bytes)` → signlink `midifade` + midisave
- `stopMidi()` → signlink `midi = "stop"`
- clientcode 3 volume: Java `midivol` **0 / -400 / -800 / -1200** (not 377’s 128/96/64/32)
- mute: `stopMidi`; unmute: re-request `nextMidiSong`
- `MIDI_SONG`: fade true, ondemand archive 2; 65535 = none
- `MIDI_JINGLE`: fade false, g2 id + g2 delay

274 and 377 share this *shape*. Volume units and fade implementation differ. This crate ports 274.

**Softsynth** is not Jagex code. Do not vendor tinymidipcm (MIDI→PCM multi-source; fights one-sequencer policy). Do not embed Spessa (`spessasynth_lib` + AudioWorklet; JS-only; used on 377 because the browser has no `javax.sound.midi`).

| Feature off | `NullMidi`: requests still complete; no device |
| Feature `audio` | one `rustysynth` sequencer + one SF2 (Florestan is an optional bank pin, not required for protocol) |

`SYNTH_SOUND` / `JagFX` / `Tone` is a separate procedural SFX path and stays in the 1:1 port.

## 5. RSA and local Server setup

Java:

```java
public static final BigInteger LOGIN_RSAN;
public static final BigInteger LOGIN_RSAE;
// assigned in <clinit> from string literals
out.rsaenc(LOGIN_RSAN, LOGIN_RSAE);
```

client-ts inlines the same pair at bundle time (`process.env.LOGIN_RSAN` / `LOGIN_RSAE`). A fresh LC engine’s `data/config/private.pem` is usually **not** that pair. Wrong public half → login **code 6**.

This crate bakes the pair at **compile** time (`build.rs` → `LOGIN_RSAN` / `LOGIN_RSAE` env, same names as the TS inject). There is no runtime pem path on `ClientConfig`. Changing the engine key requires a rebuild.

`tools/redeploy.sh` mirrors `~/redeploy.sh` / rs2b0t `tools/deploy-local-key.sh`:

1. `ENGINE_DIR` default `$HOME/experiments/Server/engine`
2. Read `$ENGINE_DIR/data/config/private.pem`
3. `openssl` modulus + public exponent → decimal `LOGIN_RSAN` / `LOGIN_RSAE`
4. `cargo build -p client-play` with those env vars (and `cargo test -p client` as needed)

If the env vars are unset at compile, use the Java client’s committed literals (LC default). That is the wrong key for a freshly generated engine pem — operators must run `tools/redeploy.sh`.

Live proof against local Server always goes through that script.

## 6. Error handling

The client reports what 274 already reports. It does not grow a bot supervisor.

| Event | Behaviour |
|---|---|
| Login success (2) | enter `gameLoop` |
| Login code 6 | `LoginError`; `client-play` prints check-pem / run `tools/redeploy.sh` |
| Other login codes | `LoginError` with code + `loginMes1` / `loginMes2` |
| Login response 1 | original: sleep 2 s, retry same `login` |
| In-game silence ~15 s | original `lostCon`: “Connection lost / reestablish”, `login(..., true)` opcode **18**; fail → `logout()` |
| Title-screen drop | no extra retry loop in `client-play` |
| TCP reset / 30 s `SoTimeout` / Isaac desync | drop stream; Java leftover state; no forged keep-alive |
| Missing jag/idx or map | `errorLoading`, `setFramerate(1)` |
| Midi missing, `audio` off | ignore (`NullMidi`) |
| Midi missing, `audio` on | warn + silence, do not crash |
| `window` / `audio` device fail | log; continue headless |
| Bad packet | Java/TS switch behaviour; no `unwrap` on untrusted bytes |

One `Client` dying must not take down other threads. `client-play` has one client and exits non-zero on login/load failure.

`NO_TIMEOUT` / idle packets stay whatever `Client` already sends.

## 7. Testing and done

### Unit (no world)

- Isaac
- CRC32
- `Packet` g/p, including RSA blob shape against a known n/e
- `ClientProt` / `ServerProt` sizes vs `~/experiments/Server/engine/src/network`
- Integer wrap fixtures (`| 0`, signed bytes)

### Live (`tools/redeploy.sh` then `client-play` + local Server)

1. Build with the **wrong** key (skip redeploy / force Java defaults against a rotated pem) → login code 6
2. `tools/redeploy.sh` then right pem → response 2, `ingame`, `REBUILD_NORMAL`, local player tile
3. After login, `tryMove` a few tiles; stream stays up (encode + Isaac)
4. Optional: `window` shows the PixMap; `audio` is not required to pass

### Not in this spec

Packet-identical golden vs the browser client, nav, bots, WebSocket, multi-account, 377 midivol, Spessa, tinymidipcm.

### Done means

`crates/client` is a severable 274 machine you can `login` and `run` without a bot. `doAction` / `tryMove` and Java-public fields exist for a later adapter. Headless is the default. RSA is baked; local Server is wired through `tools/redeploy.sh`.

## Out of scope (later specs)

- `bot-api` / `LoopingBot` / `Execution`
- nav / collision pack
- host / supervisor / N-account wall
- out-of-tree script loading
- WebSocket `ClientStream` backend
- Quest/clue content

## Key decisions (summary)

1. **Rust 1:1 port of client-ts**, Java for I/O and signlink, engine as codec oracle.
2. **Full sim, render optional** — CPU `PixMap` always; present/audio are features.
3. **No designed measurement API** — later adapter injects like rs2b0t `ClientAdapter`.
4. **20 ms GameShell loop**, not a 600 ms client tick; server tick is inbound only.
5. **TCP + compile-time RSA**; `tools/redeploy.sh` for local LC engines.
6. **274 midi knobs + rustysynth**; never tinymidipcm or Spessa.
7. **Per-`Client` mutable state** so a later host can run many accounts in one process.
