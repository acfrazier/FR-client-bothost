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

The named rows and length assertions live in `crates/client/src/io/revision.rs`;
production dispatch is in `crates/client/src/client/client.rs`.

## Source-observed but still missing dispatch

The primary dispatch contains additional branches in the startup/early-game
range that are not enabled in the R289 Rust profile. They remain fail-closed;
no numeric 274 handler is reused for them. Source-observed examples include
`12`, `28`, `46`, `60`, `71`, `79`, `81`, `82`, `83`, `87`, `90`, `91`, `106`,
`115`, `117`, `120`, `136`, `138`, `144`, `176`, `194`, `195`, `222`, `233`,
and `247`. This is an inventory of source branches, not a claim that every
opcode is emitted by the tested startup server sequence.

The next live log must therefore distinguish a new unknown packet from the
resolved opcode-13 failure. Unsupported rows continue to produce T1/logout;
no trailing-byte bypass or fabricated handler was added.

## Offline regression evidence

Command:

`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target cargo test -p client --test revision_289_stage2 -- --test-threads=1`

Result: 31 passed, 0 failed.

The new regression `startup_chat_filter_settings_289_dispatches_without_t1`
asserts the three mode values, both redraw flags, exact cursor consumption,
chat generation advancement, `ptype == -1`, and that the client remains
`ingame`. Existing stage-2 actor, region, widget, varp, login, reset, logout,
and fail-closed tests also pass.

## Limits and prerequisites

This is offline source parity only. It does not prove the live RSA/ISAAC
pairing, authentic 289 cache/server pairing, asset/render readiness, or the
full server packet sequence. Root owns further live runs and authorization.
The existing bounded branch-review limitations and the dirty inherited STATE
and helper artifacts are preserved; no live client, server, cache, account,
key, host, or other checkout was modified.
