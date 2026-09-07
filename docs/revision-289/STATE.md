# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Current task: corrective t_058241ac (strict inventory
end + immutable session revision) pending same-card reviewer acceptance after
stage-2 accepted e8ec353.
Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stage 1 accepted (ad68b99).
Stage 2 (t_da1f1a6a) accepted e8ec353 (reviewer grok-4.5, round 2). Corrective
t_058241ac in flight (implementer → same-card reviewer). Later stages remain
gated.

Serialized same-workspace dependency chain (each implementation card requires
same-card reviewer approval before the next is released):

- t_c59985d0 (luna): source/cache inventory, complete protocol contract,
  independently derived fixtures and reviewed implementation plan — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — DONE (accepted ad68b99).
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset
  — DONE (accepted e8ec353).
- t_058241ac (implementer): full inventory exact-end + session-revision API
  — pending same-card reviewer.
- t_448637e1 (implementer): cache/config, basic actions and offline replay.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

hotspot: crates/client/src/client/client.rs — campaign cards serialized.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/75/10. Known non-blocking prerequisites remain: authoritative
game-cache pairing/manifest, approved endpoint/live authorization, outbound
field/length tracing before stage-3 actions, live RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99. Fail-closed R289 inventory + framing; see prior STATE section
in git history for the full stage-1 checklist.

## Stage 2 checkpoint (t_da1f1a6a) — accepted

Commit e8ec353 (production base cc86024). Stage-2 production dispatch on R289
without replacing 274 public tables. Reviewer grok-4.5 round 2 approved.

## Corrective checkpoint (t_058241ac) — pending review

Closed three stage-1 contract gaps orch flagged after ad68b99:

1. R289 `UPDATE_INV_FULL` requires exact declared payload end before
   publication (panic → T2/logout, zero slot writes). 274 still allows trailing
   pad inside psize.
2. `adopt_from` takes the live stream first; failed handoff leaves target
   revision and partial-frame state entirely unchanged. Successful adopt still
   carries source revision with stream/ISAAC/frame baton.
3. `Client.revision` is private; bound only via `new`/`new_with_revision`,
   `from_shared`/`from_shared_with_revision`, or successful `adopt_from`.
   Read-only `revision()` getter; no unrestricted setter. 274 defaults
   unchanged on `new`/`from_shared`.

Exact tests run (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage1` → 26 passed
  (includes overlong/incomplete stream, failed adopt, immutable API)
- `cargo test -p client --test revision_289_stage2` → 28 passed
- `cargo test -p client --lib` → 65 passed
- `cargo test -p client --lib io::revision` → 2 passed
- `cargo test -p client --lib freeze_last_scene` → 1 passed
- `cargo test -p client --test from_shared --test player_info --test logout
  --test login_rsa --test gens --test prot --test server_packets`
  → 6+1+7+2+12+2+19 passed
- `cargo check -p client -p client-play` → ok

Not in this corrective (remain later / external):

- Cache/config pairing, outbound action lengths, live endpoint (stage 3)
- Live RSA/ISAAC modulus/endpoint compatibility proof
- Enabling any outbound row with `length: "unknown"`

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
