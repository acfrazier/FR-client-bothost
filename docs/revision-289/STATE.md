# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Current task: stage 1 implementation (revision
selection, production framing, inventory) pending same-card reviewer
acceptance after round-1 changes. Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stage 1 is implemented and
awaiting same-card review (round 2 after fail-closed/atomicity fixes); later
stages remain gated.

Serialized same-workspace dependency chain (each implementation card requires
same-card reviewer approval before the next is released):

- t_c59985d0 (luna): source/cache inventory, complete protocol contract,
  independently derived fixtures and reviewed implementation plan — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — implemented; pending same-card reviewer (round 2).
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

## Stage 1 checkpoint (t_3d5171fb) — pending review (round 2)

Implemented additive revision profile without replacing 274 public tables:

- `Client.revision: ClientRevision` defaults to `R274` in `Client::new` /
  `from_shared` / `construct`; opt-in `R289` at session boundary.
- `adopt_from` carries `revision` with the stream baton so size tables and
  dispatch cannot silently diverge midstream.
- `crates/client/src/io/revision.rs` + `SERVER_PROT_SIZES_289` (from
  protocol-289.json) and named `ServerProt289` inventory/logout/player/region IDs.
- Production `read_packet` / `tcp_in` select sizes via `revision.server_prot_sizes()`;
  incomplete fixed/-1/-2 frames stay pending with no partial publication.
- R289 fail-closed dispatch: `dispatch_packet_289` only runs stage-1 inventory
  full(107)/partial(76); every other inbound id is T1/unknown (no 274 handler
  side effects on collisions e.g. 47=RESET_ANIMS, 219=OBJ_REVEAL/REBUILD).
  `bump_gens` on R289 only bumps inv for those two opcodes.
- Inventory decode is psize-bounded on the production 5000-byte `in` buffer:
  full stages then publishes once; partial stages all entries then commits;
  truncated/overrun frames T2 logout with zero inventory mutation.
- 274 full/partial (106 g1 count / 172 g1 slot) and MAP_PROJANIM zone path
  unchanged on the default profile.
- Integration tests: `crates/client/tests/revision_289_stage1.rs` (manifest
  framing/inventory oracles, tcp_in production path, fragmented stream,
  production-buffer truncation, R289 collision fail-closed, adopt_from
  revision carry, 274 regressions). Generator helper:
  `tools/gen_server_prot_sizes_289.py`.

Exact tests run (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage1` → 19 passed
- `cargo test -p client --test prot --test server_packets --test gens` →
  2 + 19 + 12 passed
- `cargo test -p client --lib io::revision` → 2 passed
- `cargo test -p client --test from_shared` → 6 passed
- `cargo check -p client -p client-play` → ok

Not in this stage (remain later / external):

- Login RSA/ISAAC, actor bitstream, region/widget lifecycle (stage 2)
- Cache/config pairing, outbound action lengths, live endpoint (stage 3 / external)
- 289 logout method104 semantics beyond framing of opcode 121
- Enabling any outbound row with `length: "unknown"`

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
