# First live failure report

## Observed failure

The isolated 289 windowed run recorded `unrecognised obj config code` values
115, 175, 51 and 18, then `T1 - 13,3 - 219,-1`; the first-scene capture was
back at the title screen. Evidence is preserved outside this checkout at
`/Users/acfrazier/experiments/lostcity-289/runtime/client-proof-1737/client.log`
and `first-scene.png`.

## Source diagnosis

The 289 engine's `ObjType.decode` (`/Users/acfrazier/experiments/lostcity-289/engine/src/cache/config/ObjType.ts:185-293`)
handles code 115 as `team = dat.g1()` (`:282-283`). The Rust decoder previously
fell through for 115 without consuming its one-byte value. That explains the
apparently unrelated following values: after the missed byte, subsequent data
bytes are interpreted as opcodes (including 175, 51 and 18). This is a framing
failure, not harmless logging. The Java client uses the same packed config
contract; its cache loading is consequently not safe until the field is
consumed.

The Rust `ObjType` decoder now consumes code 115 and retains the value in a
`team` field. The field is not used by rendering, so this is behavior-preserving
for 274 while restoring the packed stream position for the 289 cache.

The Java 289 startup source also confirms that CRC loading belongs to the web
origin: `client.java:2221-2274` requests the revision-qualified CRC resource
before cache downloads. The standalone driver was assigning `Client.http_port`
after `Client::new_with_revision`, while that constructor already performed its
initial CRC probe. A new constructor binds the explicit HTTP port before that
probe; `client-play --http-port` now uses it. The existing constructor remains
the default-port compatibility wrapper, and host `from_shared` construction is
unchanged.

## Changes and verification

- `crates/client/src/config/obj_type.rs`: consume source-confirmed code 115.
- `crates/client/src/client/client.rs`: add explicit pre-construction HTTP-port
  API; preserve the old API and shared-host path.
- `crates/client-play/src/main.rs`: pass `--http-port` at construction.
- Offline verification is required before acceptance; no live client was
  launched by this task.

## Limits

The live login/scene/action/logout proof remains pending root's controlled
rerun. Authentic cache pairing, RSA/ISAAC correspondence, and real scene
readiness remain external prerequisites. This report does not claim those
proofs.
