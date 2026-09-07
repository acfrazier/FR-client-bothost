# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Current task: stage 2 implementation (login,
actors/world/widgets/lifecycle) pending same-card reviewer acceptance.
Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stage 1 accepted (ad68b99).
Stage 2 (t_da1f1a6a) implemented; awaiting same-card review. Later stages
remain gated.

Serialized same-workspace dependency chain (each implementation card requires
same-card reviewer approval before the next is released):

- t_c59985d0 (luna): source/cache inventory, complete protocol contract,
  independently derived fixtures and reviewed implementation plan — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — DONE (accepted ad68b99).
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset
  — implemented; pending same-card reviewer.
- t_448637e1 (implementer): cache/config, basic actions and offline replay.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

Corrective follow-on (orch note, not stage-2 scope): t_058241ac covers full
inventory atomicity + session-revision consistency. Stage 2 does not
independently refactor those APIs; leave that card to land after stage-2
review.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/75/10. Known non-blocking prerequisites remain: authoritative
game-cache pairing/manifest, approved endpoint/live authorization, outbound
field/length tracing before stage-3 actions, live RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99. Fail-closed R289 inventory + framing; see prior STATE section
in git history for the full stage-1 checklist.

## Stage 2 checkpoint (t_da1f1a6a) — pending review

Implemented additive stage-2 production dispatch on R289 without replacing
274 public tables or adding a parallel decoder:

- `ServerProt289` expanded with source-traced stage-2 opcodes: PLAYER_INFO
  188, NPC_INFO 65, REBUILD_NORMAL 219, LOGOUT 121, RESET_ANIMS 201,
  IF_SETTEXT 59, IF_SETANIM 211, IF_OPENMAIN_SIDE 55, IF_OPENSIDE 252,
  IF_OPENOVERLAY 127, VARP_SMALL 75, VARP_LARGE 97, VARP_SYNC 172 (plus
  stage-1 inv 107/76).
- `dispatch_packet_289` routes those through existing production handlers
  (`get_player_pos` / `get_npc_pos`, `apply_rebuild_normal`, `logout`,
  widget/varp arms). Untraced ids remain T1 fail-closed (e.g. 47).
- `apply_rebuild_normal` extracted so 274 opcode 231 and 289 opcode 219
  share one body; scene_state==1 freeze path preserved.
- Login wrapper version word is `self.revision.as_i32()` (274 default / 289
  profile); RSA plaintext order unchanged (`write_login_block`). Response
  2 vs 15 reconnect already distinct in production `login`.
- R289 `bump_gens` covers player/npc/inv/varp/stat(iface)/scene/world
  families for the traced opcodes only.
- Fixtures: manifest expanded to 20 independently derived cases (empty +
  face_entity actor, truncated player-info, empty NPC, region, widgets,
  varps, reset, login RSA structure).
- Integration tests: `crates/client/tests/revision_289_stage2.rs` (22 tests).
  Stage-1 219 collision test updated to expect rebuild (not T1) now that
  stage-2 enables it; opcode 47 remains fail-closed.

Exact tests run (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage2` → 22 passed
- `cargo test -p client --test revision_289_stage1` → 19 passed
- `cargo test -p client --test player_info --test logout --test login_rsa --lib`
  → 1 + 7 + 2 + 65 passed

Not in this stage (remain later / external):

- Cache/config pairing, outbound action lengths, live endpoint (stage 3)
- Live RSA/ISAAC modulus/endpoint compatibility proof
- Enabling any outbound row with `length: "unknown"`
- Full inventory atomicity / session-revision API cleanup (t_058241ac)

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
