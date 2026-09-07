# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Current task: stage 1 implementation (revision
selection, production framing, inventory) pending same-card reviewer
acceptance. Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stage 1 is implemented and
awaiting same-card review; later stages remain gated.

Serialized same-workspace dependency chain (each implementation card requires
same-card reviewer approval before the next is released):

- t_c59985d0 (luna): source/cache inventory, complete protocol contract,
  independently derived fixtures and reviewed implementation plan — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — implemented; pending same-card reviewer.
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset.
- t_448637e1 (implementer): cache/config, basic actions and offline replay.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/75/10. Known non-blocking prerequisites remain: authoritative
game-cache pairing/manifest, approved endpoint/live authorization, outbound
field/length tracing before stage-3 actions, live RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — pending review

Implemented additive revision profile without replacing 274 public tables:

- `Client.revision: ClientRevision` defaults to `R274` in `Client::new` /
  `from_shared` / `construct`; opt-in `R289` at session boundary.
- `crates/client/src/io/revision.rs` + `SERVER_PROT_SIZES_289` (from
  protocol-289.json) and named `ServerProt289` inventory/logout/player/region IDs.
- Production `read_packet` / `tcp_in` select sizes via `revision.server_prot_sizes()`;
  incomplete fixed/-1/-2 frames stay pending with no partial publication.
- Inventory full (289 opcode 107, g2 count) and partial (76, gsmart slot) on
  production `handle_packet` path; 274 full/partial (106 g1 count / 172 g1 slot)
  unchanged. Opcode 107 MAP_PROJANIM zone arm is 274-only so it does not steal
  289 inventory-full.
- Integration tests: `crates/client/tests/revision_289_stage1.rs` (manifest
  framing/inventory oracles, tcp_in production path, 274 regressions,
  truncated no-partial-publish). Generator helper:
  `tools/gen_server_prot_sizes_289.py`.

Exact tests run (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage1` → 12 passed
- `cargo test -p client --test prot --test server_packets --test gens` →
  2 + 19 + 12 passed
- `cargo test -p client --lib io::revision` → 2 passed
- `cargo check -p client -p client-play` → ok

Not in this stage (remain later / external):

- Login RSA/ISAAC, actor bitstream, region/widget lifecycle (stage 2)
- Cache/config pairing, outbound action lengths, live endpoint (stage 3 / external)
- 289 logout method104 semantics beyond framing of opcode 121
- Enabling any outbound row with `length: "unknown"`

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
