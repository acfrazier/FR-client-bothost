# Cleanup A report: frame and lifecycle foundation

Status: bounded offline implementation; sections B-H remain unimplemented.

## Code evidence

- `crates/client/src/io/packet.rs`: reusable packets now carry an exact frame end. Primitive reads and newline strings cannot cross that boundary, so stale bytes from a longer prior allocation are not visible to a declared frame.
- `crates/client/src/client/client.rs:3628-3690`: production `read_packet` keeps fixed zero-length frames admissible, waits for partial headers/payloads, and stamps the exact declared payload boundary before dispatch.
- `crates/client/src/client/client.rs:3699-3744`: T2/logout generation invalidation is not followed by a second success-family bump.
- `crates/client/src/client/client.rs:6800-6810,4207-4244`: UPDATE_PID remains the three-byte self-slot/membership decoder; LOGOUT remains lifecycle-owned; LAST_LOGIN_INFO decodes all ten fields, closes modal state, clears report state, and selects clientcode 650/655 without logging or resolving payload IP in packet handling.
- `logout()` resets LAST_LOGIN_INFO state through the existing lifecycle and bumps all observation families once.

The remaining R289 operations retain their existing paths. This report does not claim all 70 inbound operations are atomic or complete.

## Verification

Using `CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`:

- `cargo check -p client` — passed.
- `cargo test -p client --test packet --test revision_289_stage1` — passed, 34 tests total (7 packet, 27 stage1).
- `git diff --check` — passed.

The existing stage1 production-stream tests cover zero payload, stale backing-buffer bytes, overlong/truncated declared frames, and fragmented headers/payloads. The new packet test directly proves bounded string/read behavior; the new stage1 test proves LAST_LOGIN_INFO fields, welcome selection, modal closure, and report-state clearing.

## Boundaries and prerequisites

No host, server, vault, remote, live service, renderer ownership, or scene_state==1 freeze changes were made. Authentic 289 cache/server pairing, authorized endpoint/account, live RSA/ISAAC correspondence, and live presentation remain external prerequisites. Optional DNS display resolution remains a lifecycle/UI concern and is intentionally not performed in the packet decoder.
