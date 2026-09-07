# Revision 289 implementation plan

This plan is deliberately tied to production client paths. Standalone packet decoders do not satisfy a stage gate.

## Architecture decision

Retain all existing 274 public constants and default construction. Add explicit revision selection at the client/session boundary, carrying a revision profile into transport framing, login, inbound dispatch and outbound packet construction. Do not replace global opcodes and do not add a bot action API or host policy inside the client. Preserve renderer ownership and the `scene_state == 1` last-FBO freeze.

## Existing production bind points

- `crates/client/src/client/client.rs:291-606` owns the production `Client` state, including `ptype`, `psize`, `r#in`, `out`, `stream`, `random_in`, lifecycle flags and world/interface fields; `Client::new(ClientConfig)` is at `:849-869`, `Client::from_shared(...)` at `:877-894`, `Client::login(...)` at `:1953-1958`, and `Client::adopt_from(...)` at `:1916-1927`. Revision selection belongs in these constructors/session fields, not in global constants.
- `crates/client/src/io/client_stream.rs:71-84` defines `ClientStream`, `read_bytes` at `:204-220`, and `available` at `:229-277`; the selected profile must gate the production frame reader at the existing `Client` packet state (`ptype`/`psize`) before `handle_packet`, while preserving TCP/WSS transport behavior.
- `crates/client/src/io/server_prot.rs:4-113` exposes the public 274 `ServerProt` constants and `SERVER_PROT_SIZES`; `crates/client/src/io/client_prot.rs:4-134` exposes `ClientProt { id, length }` and outbound constants. Keep these APIs/default tables unchanged and add an explicit 289 profile beside them.
- `Client::write_login_block` (`client.rs:1935-1945`) is the existing RSA plaintext seam; `Client::login` (`:1994-2000` onward) owns handshake stream writes and `random_in`. `login_rsa.rs` remains the crypto boundary and needs a source/capture vector before 289 behavior is enabled.
- Existing inbound dispatch is in `Client::handle_packet` and its packet-family methods in `client.rs`; world/cache ownership is `Client::cache`/`ifaces`/`ifaces_mut` and `ClientBuild`. Bind 289 inventory/world/widget updates there, not in a standalone decoder. Renderer ownership remains separate; keep the `scene_state == 1` last-FBO freeze.

## Stage 1: revision profile, framing and inventory

Bind a revision profile to `Client`/session construction and make `client_stream` frame according to the selected inbound/outbound tables. Add bounded packet errors for incomplete fixed, `-1`, and `-2` frames. Wire inventory-full and inventory-partial handling through the production dispatch path, including `g2` count and `gsmart` values. Keep 274 defaults byte-for-byte compatible.

Gate: all manifest framing/inventory cases have independent oracle assertions; exact consumed lengths are checked; 274 client tests and native/client-play regressions pass; no unknown outbound row is emitted.

## Stage 2: login, actors, world, widgets and reset

Trace `login_rsa.rs`, stream setup and ISAAC seed installation against `client.java:8342-8398`. Classify response codes and make response 15/reset distinct from successful response 2. Bind actor/player update (including masks and final cursor equality), region/scene rebuild, widgets and logout/reset to the existing client lifecycle and world state. Do not claim scene readiness from socket attachment.

Gate: offline login/RSA/ISAAC vector or replay is verified; actor, region, widget and reset fixtures pass through production lifecycle; every transition is labelled PASS, FAIL or BLOCKED; renderer freeze regression remains green.

## Stage 3: cache/config, basic actions and offline end-to-end

Add an explicit cache/archive manifest and loader seam only after an authoritative 289 cache is identified. Map config/object/NPC/location/widget identities from that cache. The first action cut is existing basic client actions only (walk plus already typed entity/widget/dialog/count/logout/map paths); tutorial, guardian and random-event policy are explicitly `not impl` in this client milestone. Trace outbound payload lengths/fields from the primary Java before enabling any 289 action; unresolved outbound rows remain a fail-closed stage-3 gate. Reuse existing typed action and snapshot boundaries only where revision fixtures prove equivalent semantics.

Gate: cache hash/manifest and server pairing are recorded; each enabled outbound row has exact fields and length; offline replay covers login, player/world, inventory, scene, action and reset; missing capabilities fail closed rather than being cloned in JS.

## Review and regression gates

Each stage must pass source review before the dependent stage starts. Run client integration tests in this checkout with `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`, separately from host tests. Required regression sets include the existing 274 protocol/login/world tests, `client-play` build/startup paths and native rendering paths. Compilation, type compatibility and unchanged script declarations are not packet or lifecycle proof. Live 289 login/scene/action/reset remains BLOCKED until endpoint, credentials, compatible cache and authorized replay/live test conditions are supplied.

## Fixed decisions and external gates

The first action cut is limited to existing basic client actions; tutorial, guardian and random-event policy are `not impl`. Offline replay uses the fixture manifest JSON format: each case carries explicit initial state, opcode/frame versus payload bytes, declared length, expected decoded state/error, and cursor semantics. These are settled architecture decisions, not open alternatives.

The authoritative cache/archive and checksum/manifest, approved endpoint, credentials/test authorization, and RSA/ISAAC replay capture remain external gates. Authentic cache gates require real assets/live proof; source-derived offline loader and packet tests must not be presented as cache compatibility evidence.
