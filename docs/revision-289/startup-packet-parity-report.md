# Revision 289 startup packet parity report

Status: bounded offline source-backed update; live acceptance remains external.

## Root cause and fix

The reproduced 289 startup failure was `T1 - 13,3 - 219,-1`. The primary
289 client treats opcode 13 as the three-byte chat-filter-settings packet, not
as an unknown camera packet:

- `client.java:2612-2619`: reads three `g1` values into `anInt212`,
  `anInt234`, and `anInt360`, sets the two redraw flags, and accepts the frame.
- `client.java:5065-5066`, `5091-5092`, `5303-5304`, `5320-5321`, and
  `5338-5339`: consume those values as public/private/trade chat visibility
  modes.
- `Class17.java:11`: the 289 packet-length table gives opcode 13 length 3.

The R289 profile now names opcode 13 `CHAT_FILTER_SETTINGS`, dispatches it
through the existing production `apply_chat_filter_settings` implementation,
bumps the chat generation, and preserves exact three-byte consumption. The
protocol inventory was corrected from the former ambiguous
`camera_or_scene_triplet` label to `chat_filter_settings`.

Opcode 219 was already correctly source-backed as `REBUILD_NORMAL`: the
primary client reads two `g2` values and starts the map rebuild at
`client.java:2999-3022`; the R289 table length is 4 and the Rust dispatch is
already wired to `apply_rebuild_normal`.

## Covered R289 dispatch inventory

The explicit R289 dispatch currently covers these source-anchored families:

- `13` chat filter settings: three `g1` mode values; redraw chat and mode.
- `23` IF_CLOSE: clears side/chat/name-entry/main interface modals.
- `47` UPDATE_IGNORELIST: consumes repeated `g8` hashes.
- `120` UPDATE_PID: `g2` self slot plus `g1` members flag.
- `235` FRIENDLIST_LOADED: consumes the social-server status byte.
- `65` NPC_INFO: `method187` actor update path.
- `75` VARP_SMALL: `g2` id plus signed `g1` value.
- `76` UPDATE_INV_PARTIAL: gsmart slot form.
- `97` VARP_LARGE: `g2` id plus `g4` value.
- `107` UPDATE_INV_FULL: `g2` component plus `g2` entry count.
- `121` LOGOUT: full lifecycle reset.
- `127` IF_OPENOVERLAY: signed `g2` component.
- `172` VARP_SYNC: bulk server-to-client varp copy.
- `188` PLAYER_INFO: actor bitstream and exact frame-end check.
- `201` RESET_ANIMS: clears primary animations.
- `211` IF_SETANIM: `g2` component plus signed `g2` sequence.
- `219` REBUILD_NORMAL: `g2`/`g2` region base and scene-state transition.
- `252` IF_OPENSIDE: `g2` component.
- `59` IF_SETTEXT: `g2` component plus newline string.

The source-ordered offline regression `startup_289_source_sequence_keeps_stream_in_game`
feeds the covered prefix as exact frames: `219` rebuild, `13` chat modes,
`172` varp sync, `75`/`97` varp updates, `107` empty inventory, and `201`
reset animations. It asserts every fixed payload cursor, `scene_state == 1`
after rebuild, and that the client remains ingame.

The named rows and length assertions live in `crates/client/src/io/revision.rs`;
production dispatch is in `crates/client/src/client/client.rs`.

## Isolated-engine startup audit

The authorized isolated engine's `Player.onLogin` source emits the following
concrete sequence. Its packet IDs and lengths match the authentic 289 table;
the Rust profile now dispatches each packet without numeric aliases:

| `Player.onLogin` emission | Engine ID/length | R289 handler | Rust state/effect |
|---|---:|---|---|
| `rebuildNormal()` | 219/4 | `REBUILD_NORMAL` | region base and scene loading |
| `ChatFilterSettings` | 13/3 | `CHAT_FILTER_SETTINGS` | public/private/trade modes |
| friend enabled/disabled `FriendlistLoaded` | 235/1 | `FRIENDLIST_LOADED` | friend-server status |
| disabled `UpdateIgnoreList([])` | 47/-2 | `UPDATE_IGNORELIST` | clears/loads ignore hashes |
| `IfClose` | 23/0 | `IF_CLOSE` | closes interface modals |
| `UpdatePid` | 120/3 | `UPDATE_PID` | local slot and members flag |
| `ResetClientVarCache` | 172/0 | `VARP_SYNC` | applies authoritative var cache |
| transmitted `writeVarp` values | 75/3 or 97/6 | `VARP_SMALL`/`VARP_LARGE` | applies varp and client-var effects |
| `ResetAnims` | 201/0 | `RESET_ANIMS` | clears actor primary animations |

Anchors: absolute isolated-engine `engine/src/engine/entity/Player.ts:488-527`
documents and emits the login order; `engine/src/network/game/server/ServerGameProt.ts:3-83`
defines the IDs and lengths; `Player.ts:516-521` and `:1806-1811` select the
transmitted varp forms. The pinned Java dispatch confirms the corresponding
semantics at `client.java:2612-2619`, `2648-2652`, `2819-2823`,
`2833-2837`, `3351-3383`, `3402-3415`, `3471-3508`, and `3518-3539`.

The source-ordered regression now feeds this complete concrete login prefix,
including social, identity, var-cache and animation-reset frames. Script-driven
packets after `onLogin` are not claimed: they depend on runtime script/provider
state and must be audited from their emitting scripts before adding coverage.

## Source-observed but still missing dispatch

Additional primary-client branches remain fail-closed because they are not
emitted by the concrete `Player.onLogin` sequence and lack a bounded startup
emission trace. Examples include `12`, `28`, `46`, `60`, `71`, `79`, `81`, `82`,
`83`, `87`, `90`, `91`, `106`, `115`, `117`, `136`, `138`, `144`, `154`, `155`,
`176`, `194`, `195`, `222`, `233`, and `247`. This is an honest non-startup
inventory, not a claim that every opcode is required before scene readiness.

The next live log must therefore distinguish a new unknown packet from the
resolved opcode-13 failure, while also recording whether the endpoint is using
the engine table or the authentic 289 table. Unsupported rows continue to
produce T1/logout; no trailing-byte bypass or fabricated handler was added.

## Offline regression evidence

Command:

`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --test revision_289_stage2 -- --test-threads=1`

Result: 33 passed, 0 failed.

The regressions `startup_chat_filter_settings_289_dispatches_without_t1`,
`startup_289_source_sequence_keeps_stream_in_game`, and
`startup_289_engine_login_social_and_identity_packets_dispatch`
assert the three mode values, both redraw flags, exact cursor consumption,
chat generation advancement, the ordered startup state transitions, fixed
payload cursors, `ptype == -1`, and that the client remains `ingame`. Existing
stage-2 actor, region, widget, varp, login, reset, logout, and fail-closed tests
also pass.

## Limits and prerequisites

This is offline source parity only. It does not prove the live RSA/ISAAC
pairing, authentic 289 cache/server pairing, asset/render readiness, or the
full server packet sequence. Script/provider-driven packets after `onLogin`
remain outside this bounded audit. Root owns further live runs and
authorization.
The existing bounded branch-review limitations and the dirty inherited STATE
and helper artifacts are preserved; no live client, server, cache, account,
key, host, or other checkout was modified.
