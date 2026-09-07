# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Current task: first milestone and deliverables (source,
cache inventory, complete protocol/lifecycle mapping, offline fixtures, reviewed
implementation dependency plan). Continue subsequent client stages after the
plan's actual reviewer approval; do not stop at a plan when independent client
implementation can proceed. Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. No 289 implementation or proof has run yet. The live
289 server/cache pairing is unverified; investigate existing source/artifact
provenance read-only, report exact remaining prerequisites, and do not invent
a successful live target. Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation is not yet accepted.
Serialized same-workspace dependency chain (each implementation card requires
same-card reviewer approval before the next is released):

- t_c59985d0 (luna): source/cache inventory, complete protocol contract,
  independently derived fixtures and reviewed implementation plan.
- t_3d5171fb (implementer): revision selection, production framing/inventory.
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset.
- t_448637e1 (implementer): cache/config, basic actions and offline replay.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json. No numeric mappings
are approved until the primary Java evidence and plan pass review.

Verified here: clean git status and codex/revision-289-client branch;
`hermes profile list` reports expected model names for all five profiles.
No client tests or implementation have run in this orchestration checkpoint.
A read-only Python configuration inspection was denied by command approval;
no approval/auth/provider settings were changed. Provider defaults remain
task-supplied evidence; actual review model/provider receipts must be checked.

The source inventory explicitly identifies client/deob JARs, not a verified
game-cache pairing. Authorized endpoint, compatible game-cache manifest and
live credentials/test authorization remain unverified, not reasons to skip
independent offline work. Parent Codex retains integration/live coordination.
Next: release source milestone, obtain actual plan review, then execute the
serialized production stages and final branch review. Dispatch is not proof of
execution, successful tests, review acceptance or a complete 289 port.

## Source milestone checkpoint (t_c59985d0, revision 3)

Deliverables prepared for same-card independent review after correcting the first review's primary-source and oracle findings and completing the actor/login gaps:

- `docs/revision-289/source-contract.md`
- `docs/revision-289/protocol-289.json` (256 inbound rows; 75 observed outbound IDs, unresolved lengths explicit)
- `docs/revision-289/implementation.md`
- `crates/client/tests/fixtures/revision_289/manifest.json` (10 public-safe cases with frame/payload separation, declared length, cursor, structured result, valid empty actor bitstream, and credential-free login plaintext structure)
- `tools/verify_revision_289_contract.py` (shape, uniqueness, opcode/length-kind, declared-length, cursor-consumption and oracle-word verifier)
- `tools/generate_revision_289_contract.py` (reproducible source extraction for the pinned read-only vault checkout and source-traced fixture generation, including method212/185/172/153/128 actor fields)

Exact verification run: `python3 tools/generate_revision_289_contract.py && python3 tools/verify_revision_289_contract.py && git diff --check` -> `PASS: 256 inbound, 75 outbound rows; 10 fixtures`.

Primary-source corrections recorded: inventory full is 107, partial is 76 with gsmart slots, logout is 121, region is 219, varp small/large are 75/97, 172 is bulk varp sync, 55 is dual-interface open, 127 is signed-g2 interface state, and 211 is component animation. Actor fields now trace method212/185/172/153/128, with a valid two-byte zero actor bitstream; login plaintext order and revision/cache-index frame fields trace client.java:8342-8398, with credentials and RSA constants excluded.

Known blockers remain honest: no authoritative game-cache pairing/manifest, approved endpoint or live authorization; outbound field/length tracing and live RSA/ISAAC compatibility proof are still required. No client implementation, live test, host test, push, merge, remote or submodule change was performed in this milestone. Same-card reviewer must independently inspect primary Java and fixture derivations and record actual model/provider, commit and verdict before releasing t_3d5171fb.

## Source milestone checkpoint (t_c59985d0, revision 4)

Corrected the final reviewer finding in the generated contract: player-update
mask dispatch now anchors method153 at `client.java:7207-7218`, replacing the
unrelated NPC method124 range; the actor fixture carries
the same direct method153 anchor. Regenerated protocol-289.json and the fixture
manifest from the pinned read-only source.

Exact verification: `python3 tools/generate_revision_289_contract.py &&
python3 tools/verify_revision_289_contract.py && git diff --check` -> `PASS:
256 inbound, 75 outbound rows; 10 fixtures`. Pending same-card reviewer
approval and recording of the actual review model/provider and verdict; no
implementation, live, host, push, merge, remote or submodule work performed.
