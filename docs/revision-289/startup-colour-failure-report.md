# Revision 289 startup colour failure report

Status: bounded offline source-backed correction; live acceptance remains external.

## Reproduced failure

The approved `1c1c2fb` client proof reached `ingame` and then logged `T1 - 160,4 - 63,63`. The preserved root evidence is `/Users/acfrazier/experiments/lostcity-289/runtime/client-proof-1844/client.log` with the corresponding `after-login.png`. The failure is a protocol dispatch miss, not a permission to skip bytes or continue after an unknown packet.

## Root cause

The isolated engine defines `ServerGameProt.IF_SETCOLOUR` as opcode 160 with a four-byte payload (`/Users/acfrazier/experiments/lostcity-289/engine/src/network/game/server/ServerGameProt.ts:13`). Its encoder writes `p2(component)` then `p2(colour)` (`engine/src/network/game/server/codec/IfSetColourEncoder.ts:9-11`). The script path is transitive and concrete:

`login.rs2:80-96` -> `~initalltabs` and `~update_questlist` -> `quests.rs2:301-378` -> `send_quest_progress_colour` -> `if_setcolour` (`quests.rs2:1-13`) -> opcode 160.

The primary 289 Java client decodes opcode 160 at `client.java:3362-3370`: it reads component g2 and RGB555 colour g2, expands the five-bit channels to the 24-bit interface colour `(r << 19) + (g << 11) + (b << 3)`. The previous R289 dispatch did not recognize 160, so the actual startup stream logged T1 and logged out after the script-driven initialization packets.

## Correction

- Added `ServerProt289::IF_SETCOLOUR = 160` and the R289 size assertion.
- Corrected R289 framing table entry 160 to length 4.
- Added fail-closed production dispatch with exact two-g2 consumption and per-client `IfTypeMut::colour` state update. Missing component slots remain safe no-ops while the payload is still consumed.
- Promoted protocol row 160 from fields-unknown to source-verified fields and anchors.
- Added an ordered production regression covering the login script path through `IF_SETTAB`, `IF_SETCOLOUR`, stat, energy, weight, identity, zone and inventory packets. It asserts exact cursor consumption, RGB555 expansion, observable interface state, and that the client remains ingame. A focused regression independently covers the colour packet.

## Audit boundary

The same exact source trace confirms the login trigger's other concrete output families: welcome message, camera reset, minimap toggle, player options, interface tabs, inventory transmission, last-login information, and first-tick stat/energy/weight, actor and zone packets. Those existing R289 handlers remain in the ordered regression. `update_weapon_category` emits interface object/text/hide/tab packets only conditionally on the worn weapon; `update_all` emits stat/appearance/weight side effects; timers and queued procedures are not promoted to startup packets because their runtime conditions are not established by the login trace. No unsupported trailing-byte bypass, fake handler, or unknown-packet skip was added.

## Verification

Commands used `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target` where applicable:

- Focused colour regression: 1 passed, 0 failed.
- `cargo test -p client --test revision_289_stage2 -- --test-threads=1`: 36 passed, 0 failed.
- `cargo test -p client --test revision_289_stage1 -- --test-threads=1`: 26 passed, 0 failed.
- `python3 tools/verify_revision_289_contract.py`: PASS, 256 inbound / 82 outbound / 50 fixtures.
- `cargo check -p client -p client-play`: passed.
- `git diff --check`: passed.

## Limits and preserved evidence

This is offline protocol/state proof only. It does not claim live RSA/ISAAC correspondence, authentic cache/server pairing, scene readiness, or live login acceptance. The failed `client-proof-1844` artifacts remain preserved outside this checkout. No live client/server/cache/profile/source fixture or remote host was modified.
