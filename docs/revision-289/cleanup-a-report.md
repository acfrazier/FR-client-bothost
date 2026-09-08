# Cleanup A report: frame and lifecycle foundation

Status: bounded offline implementation; sections B-H remain unimplemented.

## Code evidence

- `crates/client/src/io/packet.rs`: reusable packets now carry an exact frame end. Primitive reads and newline strings cannot cross that boundary, so stale bytes from a longer prior allocation are not visible to a declared frame.
- `crates/client/src/client/client.rs:3628-3690`: production `read_packet` keeps fixed zero-length frames admissible, waits for partial headers/payloads, and stamps the exact declared payload boundary before dispatch.
- `crates/client/src/client/client.rs:3699-3748`: R289 publication is selected from the successful semantic opcode, while the established 274 before/after publication path is retained; T2/logout invalidation is not followed by a second success-family bump.
- `crates/client/src/client/client.rs:6800-6810,4207-4248`: UPDATE_PID remains the three-byte self-slot/membership decoder; LOGOUT remains lifecycle-owned; LAST_LOGIN_INFO stages all ten bytes before mutation, follows Java's `(recovery != 201 || members_warning == 1) ? 655 : 650`, uses `layer_id`, closes modal state, and clears report state only inside the Java welcome gate. No IP is logged or resolved in packet handling.
- `logout()` resets LAST_LOGIN_INFO state through the existing lifecycle and bumps all observation families once.

The remaining R289 operations retain their existing paths. This report does not claim all 70 inbound operations are atomic or complete.

## Verification

Using `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`:

- `cargo check -p client` — passed.
- `cargo test -p client --test revision_289_stage1 --test packet --test gens --test logout --test server_packets` — passed, 74 tests total (29 stage1, 7 packet, 12 gens, 7 logout, 19 server_packets).
- `git diff --check` — passed.

The stage1 production-stream tests cover zero payload, stale backing-buffer bytes, overlong/truncated declared frames, and fragmented headers/payloads. The packet test directly proves bounded string/read behavior; stage1 tests cover both Java welcome branches, layer-vs-component identity, modal/report gate behavior, and R289 publication. The remaining R289 operations retain their existing non-atomic paths.

## Boundaries and prerequisites

No host, server, vault, remote, live service, renderer ownership, or scene_state==1 freeze changes were made. Authentic 289 cache/server pairing, authorized endpoint/account, live RSA/ISAAC correspondence, and live presentation remain external prerequisites. Optional DNS display resolution remains a lifecycle/UI concern and is intentionally not performed in the packet decoder.
