# Revision 289 startup packet parity report

Status: bounded offline source-backed startup coverage; live acceptance remains external.

## Sources and provenance

- Authentic 289 Java dispatch: `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/client.java`.
- Isolated engine login emission: `/Users/acfrazier/experiments/lostcity-289/engine/src/engine/entity/Player.ts:488-533`.
- Isolated engine packet IDs and lengths: `/Users/acfrazier/experiments/lostcity-289/engine/src/network/game/server/ServerGameProt.ts:1-89`.
- Isolated engine zone packet IDs and lengths: `/Users/acfrazier/experiments/lostcity-289/engine/src/network/game/server/ServerGameZoneProt.ts:1-15`.
- Isolated login script: `/Users/acfrazier/experiments/lostcity-289/content/scripts/login_logout/login.rs2:1-116`.
- Isolated first-tick output: `/Users/acfrazier/experiments/lostcity-289/engine/src/engine/entity/NetworkPlayer.ts:155-189,286-395`.

No source outside these pinned absolute paths was used for the startup audit. The inherited untracked helper/review artifacts remain untouched and unstaged.

## Root cause and corrected dispatch

The reproduced stream was `T1 - 13,3 - 219,-1`. Authentic 289 dispatch reads opcode 13 as three `g1` chat visibility modes (`client.java:2612-2619`) and the authentic length table gives it length 3 (`Class17.java:11`). Opcode 219 is the four-byte `REBUILD_NORMAL` packet (`client.java:2999-3022`).

R289 dispatch now covers both packets without numeric aliases, preserves exact payload consumption, bumps the appropriate generation state, and leaves the existing default R274 table unchanged.

## Concrete startup emissions and coverage

### Player.onLogin ordered prefix

`Player.ts:488-527` emits this exact order. IDs and lengths are from `ServerGameProt.ts`.

| Order | Engine message | ID / length | Rust R289 coverage |
|---:|---|---:|---|
| 1 | `REBUILD_NORMAL` | 219 / 4 | region base, scene loading |
| 2 | `CHAT_FILTER_SETTINGS` | 13 / 3 | public/private/trade modes |
| 3a | `FRIENDLIST_LOADED` (friend enabled) | 235 / 1 | social status |
| 3b | `FRIENDLIST_LOADED` (friend disabled) | 235 / 1 | social status |
| 3c | `UPDATE_IGNORELIST([])` (friend disabled) | 47 / -2 | empty/repeated g8 hashes |
| 4 | `IF_CLOSE` | 23 / 0 | closes modals |
| 5 | `UPDATE_PID` | 120 / 3 | self slot and members flag |
| 6 | `RESET_CLIENT_VARCACHE` | 172 / 0 | authoritative var cache copy |
| 7 | `writeVarp` for transmitted vars | 75 / 3 or 97 / 6 | small/large varp state |
| 8 | `RESET_ANIMS` | 201 / 0 | clears actor primary animations |

The social/identity/var-cache sequence is covered by `startup_289_engine_login_social_and_identity_packets_dispatch` and the ordered prefix regression in `revision_289_stage2.rs`.

### LOGIN trigger script

`login.rs2:1-90` is the concrete LOGIN trigger selected by `Player.onLogin:525-527`. Its emitted packet coverage is:

| Script source | Engine message | ID / length | Rust R289 coverage |
|---|---|---:|---|
| `mes("Welcome...")` | `MESSAGE_GAME` | 196 / -1 | chat insertion and exact jstr |
| `cam_reset` (twice) | `CAM_RESET` | 133 / 0 | camera/shake reset |
| `minimap_toggle(0)` | `MINIMAP_TOGGLE` | 136 / 1 | minimap state |
| `set_player_op(...)` (multiple) | `SET_PLAYER_OP` | 21 / -1 | option text/priority |
| wilderness overlay branch | `IF_OPENOVERLAY` | 127 / 2 | existing widget handler; conditional only |
| `initalltabs` / `if_settab(...)` | `IF_SETTAB` | 63 / 3 | side-tab component mapping |
| `inv_transmit(...)` | `UPDATE_INV_FULL` | 107 / -2 | existing inventory decoder |
| `last_login_info` when `map_live` | `LAST_LOGIN_INFO` | 253 / 10 | fixed five-field payload consumption |

The script also schedules timers and queued procedures. Those are not packet emissions until their runtime conditions execute; they are not fabricated into the startup sequence.

### First tick / NetworkPlayer output

`NetworkPlayer.ts:317-330` emits changed stats, run energy, and `NetworkPlayer.ts:332-395` emits first-seen inventory and run weight. `NetworkPlayer.ts:286-313` emits player/NPC and zone bootstrap. R289 coverage now includes:

- `UPDATE_STAT` 154/6: stat id, g4 experience, effective level; redraw and base-level derivation.
- `UPDATE_RUNENERGY` 195/1: energy byte and stats-tab redraw.
- `UPDATE_RUNWEIGHT` 46/2: signed g2 and stats-tab redraw.
- `PLAYER_INFO` 188/-2 and `NPC_INFO` 65/-2: existing exact actor decoders.
- `UPDATE_ZONE_PARTIAL_FOLLOWS` 155/2 and `UPDATE_ZONE_FULL_FOLLOWS` 144/2: zone origin and full-zone object invalidation.
- `UPDATE_ZONE_PARTIAL_ENCLOSED` 112/-2: zone origin and R289 inner-zone dispatch.
- `UPDATE_INV_FULL` 107/-2 and `UPDATE_INV_PARTIAL` 76/-2: existing inventory paths.

The regressions `startup_289_source_sequence_keeps_stream_in_game`,
`startup_289_login_script_and_first_tick_packets_dispatch`, and
`startup_289_enclosed_zone_uses_289_inner_opcodes_and_keeps_framing` exercise the
ordered onLogin prefix, LOGIN script, first-tick stats/identity, and a non-empty
R289 enclosed-zone frame. Inner IDs are translated from the isolated
`ServerGameZoneProt.ts:5-14` table to the shared field-width implementations;
unknown inner IDs consume the remainder of their outer frame rather than
desynchronizing the next top-level packet.

## Covered R289 inventory

Source-anchored production dispatch currently covers: 13, 21, 23, 46, 47, 55, 59, 63, 65, 75, 76, 97, 107, 112, 120, 121, 127, 133, 136, 144, 154, 155, 172, 188, 195, 196, 201, 211, 219, 235, and 252. Enclosed R289 zone IDs 83, 60, 71, 176, 90, 87, 194, 117, 233, and 106 are covered by the inner dispatch. All named rows assert their exact lengths against `SERVER_PROT_SIZES_289`.

## Still fail-closed

Packets not emitted by the concrete onLogin/login-trigger/first-tick trace remain fail-closed. This includes unrelated primary-client branches such as tutorial, audio, arbitrary interface updates, and private messages. Unknown top-level IDs still report T1 and invoke the existing logout path; unknown inner-zone IDs consume only their enclosing frame remainder. No trailing-byte checks were bypassed and no fabricated handler or foreign runtime was added.

## Verification

- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --test revision_289_stage2 -- --test-threads=1` — 35 passed, 0 failed.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --test revision_289_stage1 --test revision_289_stage2 --lib -- --test-threads=1` — 73 lib, 26 stage1, 35 stage2 passed; 0 failed.
- `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo check -p client -p client-play` — passed.
- `python3 tools/verify_revision_289_contract.py` — PASS: 256 inbound, 82 outbound rows; 50 fixtures.
- `git diff --check` — passed.

## Limits

This is offline source parity only. It does not prove live RSA/ISAAC correspondence, authentic cache/server pairing, asset/render readiness, scene readiness, or live login/action/logout. Root retains live authorization and acceptance ownership. No live client, server, cache, account, key, host, isolated engine, or other checkout was modified.
