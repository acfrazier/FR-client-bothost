# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Current task: stage 2 implementation (login,
actors/world/widgets/lifecycle) pending same-card reviewer acceptance
(round-2 after changes-requested on cc86024).
Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stage 1 accepted (ad68b99).
Stage 2 (t_da1f1a6a) rework after round-1 changes-requested; awaiting
same-card reviewer (profile `reviewer`, top-level assignee — not implementer
self-review). Later stages remain gated.

Serialized same-workspace dependency chain (each implementation card requires
same-card reviewer approval before the next is released):

- t_c59985d0 (luna): source/cache inventory, complete protocol contract,
  independently derived fixtures and reviewed implementation plan — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — DONE (accepted ad68b99).
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset
  — rework complete; pending same-card reviewer.
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

hotspot: crates/client/src/client/client.rs — campaign cards serialized.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/75/10. Known non-blocking prerequisites remain: authoritative
game-cache pairing/manifest, approved endpoint/live authorization, outbound
field/length tracing before stage-3 actions, live RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99. Fail-closed R289 inventory + framing; see prior STATE section
in git history for the full stage-1 checklist.

## Stage 2 checkpoint (t_da1f1a6a) — pending review (round 2)

Implemented additive stage-2 production dispatch on R289 without replacing
274 public tables or adding a parallel decoder (base cc86024), plus round-1
acceptance proofs:

- `ServerProt289` stage-2 opcodes: PLAYER_INFO 188, NPC_INFO 65, REBUILD_NORMAL
  219, LOGOUT 121, RESET_ANIMS 201, IF_SETTEXT 59, IF_SETANIM 211,
  IF_OPENMAIN_SIDE 55, IF_OPENSIDE 252, IF_OPENOVERLAY 127, VARP_SMALL 75,
  VARP_LARGE 97, VARP_SYNC 172 (plus stage-1 inv 107/76).
- `dispatch_packet_289` routes those through existing production handlers.
  Untraced ids remain T1 fail-closed (e.g. 47).
- `apply_rebuild_normal` shared 274/289; scene_state==1 freeze preserved.
- Login wrapper version word is `self.revision.as_i32()`; production `login()`
  path proven offline for outer frame 16|size|255|p2(289)|lowmem|checksums|RSA
  and Isaac install (out.random + random_in seed+50).
- Response 2 vs 15 distinct on R289 (cold clears players/npcs/scene_state;
  reconnect keeps local/players/npcs/scene_state).
- Lifecycle hard gate: attached (stream) ≠ ingame ≠ scene_ready (scene_state==2
  only via check_scene success path; rebuild leaves scene_state==1).
- Actor removals: PLAYER_INFO and NPC_INFO empty old-vis clear prior slots via
  production entity_removal.
- NPC non-empty: FACEENTITY mask fixture through get_npc_pos.
- Exact payload cursor == psize on stage-2 handlers; tcp_in consumes manifest
  frame lengths for representative opcodes.
- `sizes_289_named_rows` extended to all stage-2 named opcodes.
- Fixtures: manifest expanded with removal/NPC mask/login outer-frame cases.
- Integration tests: `revision_289_stage2.rs` (28 tests).

Exact tests run (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage2` → 28 passed
- `cargo test -p client --test revision_289_stage1` → 19 passed
- `cargo test -p client --lib io::revision` → 2 passed
- `cargo test -p client --test player_info --test logout --test login_rsa`
  → 1 + 7 + 2 passed
- `cargo check -p client -p client-play` → ok

Not in this stage (remain later / external):

- Cache/config pairing, outbound action lengths, live endpoint (stage 3)
- Live RSA/ISAAC modulus/endpoint compatibility proof
- Enabling any outbound row with `length: "unknown"`
- Full inventory atomicity / session-revision API cleanup (t_058241ac)

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
