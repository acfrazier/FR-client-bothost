# Revision 289 implementation plan

This plan is deliberately tied to production client paths. Standalone packet decoders do not satisfy a stage gate.

## Architecture decision

Retain all existing 274 public constants and default construction. Add explicit revision selection at the client/session boundary, carrying a revision profile into transport framing, login, inbound dispatch and outbound packet construction. Do not replace global opcodes and do not add a bot action API or host policy inside the client. Preserve renderer ownership and the `scene_state == 1` last-FBO freeze.

## Stage 1: revision profile, framing and inventory

Bind a revision profile to `Client`/session construction and make `client_stream` frame according to the selected inbound/outbound tables. Add bounded packet errors for incomplete fixed, `-1`, and `-2` frames. Wire inventory-full and inventory-partial handling through the production dispatch path, including `g2` count and `gsmart` values. Keep 274 defaults byte-for-byte compatible.

Gate: all manifest framing/inventory cases have independent oracle assertions; exact consumed lengths are checked; 274 client tests and native/client-play regressions pass; no unknown outbound row is emitted.

## Stage 2: login, actors, world, widgets and reset

Trace `login_rsa.rs`, stream setup and ISAAC seed installation against `client.java:8342-8398`. Classify response codes and make response 15/reset distinct from successful response 2. Bind actor/player update (including masks and final cursor equality), region/scene rebuild, widgets and logout/reset to the existing client lifecycle and world state. Do not claim scene readiness from socket attachment.

Gate: offline login/RSA/ISAAC vector or replay is verified; actor, region, widget and reset fixtures pass through production lifecycle; every transition is labelled PASS, FAIL or BLOCKED; renderer freeze regression remains green.

## Stage 3: cache/config, basic actions and offline end-to-end

Add an explicit cache/archive manifest and loader seam only after an authoritative 289 cache is identified. Map config/object/NPC/location/widget identities from that cache. Trace outbound payload lengths/fields from the primary Java before enabling walk, NPC/loc/object/player/held/widget, dialog/count, logout and map actions. Reuse existing typed action and snapshot boundaries only where revision fixtures prove equivalent semantics.

Gate: cache hash/manifest and server pairing are recorded; each enabled outbound row has exact fields and length; offline replay covers login, player/world, inventory, scene, action and reset; missing capabilities fail closed rather than being cloned in JS.

## Review and regression gates

Each stage must pass source review before the dependent stage starts. Run client integration tests in this checkout with `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`, separately from host tests. Required regression sets include the existing 274 protocol/login/world tests, `client-play` build/startup paths and native rendering paths. Compilation, type compatibility and unchanged script declarations are not packet or lifecycle proof. Live 289 login/scene/action/reset remains BLOCKED until endpoint, credentials, compatible cache and authorized replay/live test conditions are supplied.

## Decisions needing parent confirmation

- authoritative cache/archive and checksum/manifest;
- approved endpoint and replay/live authorization;
- whether the first action cut is limited to walk plus basic entity/widget actions, with tutorial/guardian/random-event policy explicitly `not impl`;
- accepted offline replay format for RSA/ISAAC and lifecycle evidence.
