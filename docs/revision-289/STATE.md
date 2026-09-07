# Revision 289 client campaign state

Authorized 2026-09-07. Prepared branch codex/revision-289-client, base
4f2048ea10f75b3bb92ff45610b35ba7313b0308 (published r274-bh-modular).
Current plan: plan.md. Stage 3 (t_448637e1) implementation complete; pending
same-card reviewer. Required final branchreviewer remains.

Primary source pin and supporting host evidence are named in plan.md and
host-script-evidence.md. The live 289 server/cache pairing is unverified.
Only this checkout may be edited or built by workers.

## Dispatch checkpoint

Orchestration card: t_95bef768. Implementation stages 1–2 accepted. Corrective
t_058241ac accepted f766c9a. Stage 3 t_448637e1 in review.

Serialized same-workspace dependency chain:

- t_c59985d0 (luna): source/cache inventory, complete protocol contract — DONE
  (approved round 4 on d2c6318, model grok-4.5).
- t_3d5171fb (implementer): revision selection, production framing/inventory
  — DONE (accepted ad68b99).
- t_da1f1a6a (implementer): login, actors/world/widgets and lifecycle reset
  — DONE (accepted e8ec353).
- t_058241ac (implementer): full inventory exact-end + session-revision API
  — DONE (accepted f766c9a).
- t_448637e1 (implementer): cache/config, basic actions and offline replay
  — pending same-card reviewer.
- t_77d35bfb (branchreviewer): required independent Grok4.6 branch verdict.

Design boundary: retain 274 public constants and default construction; add
explicit revision selection at the client/session boundary. Source contract
artifacts: source-contract.md, protocol-289.json, implementation.md and
crates/client/tests/fixtures/revision_289/manifest.json.

hotspot: crates/client/src/client/client.rs — campaign cards serialized.

## Source milestone (accepted)

Source contract approved on commit d2c6318 (reviewer grok-4.5, round 4).
Verifier PASS 256/75/10. Known non-blocking prerequisites remain: authoritative
game-cache pairing/manifest, approved endpoint/live authorization, live
RSA/ISAAC compatibility proof.

## Stage 1 checkpoint (t_3d5171fb) — accepted

Commit ad68b99.

## Stage 2 checkpoint (t_da1f1a6a) — accepted

Commit e8ec353 (production base cc86024). Reviewer grok-4.5 round 2 approved.

## Corrective checkpoint (t_058241ac) — accepted

Commit f766c9a. Strict INV_FULL end on R289; adopt_from fail-closed; private
revision construction API.

## Stage 3 checkpoint (t_448637e1) — pending review

Source-traced outbound action cut + offline cache seam + offline replay.

Production changes:

1. `ClientProt289` + `map_client_prot` (R274 identity / R289 remap, fail-closed
   on unmapped 274 constants). IDs and payload lengths for walk, NPC, loc,
   object, player, held, inv-button, IF_BUTTON, resume pause/count, close
   modal, map_build, chat_setmode, no_timeout traced from primary 289
   `client.java` `method465` / `method160` / `method206` write sequences
   (method466=p1, method467=p2, method470=p4). Shapes match 274 for this cut;
   opcodes differ. Public `ClientProt` 274 table unchanged.
2. Production emit: `Client::client_opcode` routes every outbound
   `p1_enc` / interact opcode through the session revision mapper (83 sites).
3. Offline cache/config seam `io/cache_289.rs`: login JAG name/CRC slot layout,
   `CacheManifest289`, `load_offline_config_seam`, `synthetic_jag` (public-safe
   fixture builder). Explicit `authentic_cache_present=false`; missing
   authentic cache does not claim scene/assets.
4. Offline native replay tests: receive (logout/varp/widget/reset/inv-full) →
   lifecycle → action emit (walk/NPC) → scene_state stays 1 without map data
   (last-FBO freeze band preserved).
5. Manifest outbound/cache cases appended with source anchors and provenance.

Exact tests (CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target):

- `cargo test -p client --test revision_289_stage3` → 19 passed
- `cargo test -p client --lib` → 69 passed (includes io::cache_289 ×4)
- `cargo test -p client --test revision_289_stage1` → 26 passed
- `cargo test -p client --test revision_289_stage2` → 28 passed
- `cargo test -p client --test do_action --test walk --test prot` → 13+6+2
- `cargo test -p client --test logout --test from_shared --test player_info
  --test login_rsa --test gens --test server_packets` → 7+6+1+2+12+19
- `cargo test -p client --lib freeze_last_scene` → 1 passed
- `cargo check -p client -p client-play` → ok

PASS (offline):

- R289 MOVE_GAMECLICK=234 / MINIMAP=236 / OPCLICK=67 with source payload shapes
- OPNPC2=21 len2, OPLOC1=10 len6, OPHELD1=76 len6, INV_BUTTON1=44 len6,
  IF_BUTTON=86 len2, RESUME_PAUSE=166 len2, RESUME_P_COUNT=180 len4,
  CLOSE_MODAL=93 len0, MAP_BUILD_COMPLETE=214, NO_TIMEOUT=181
- R274 default emit still uses public ClientProt ids (e.g. MOVE_GAMECLICK 207)
- Offline synthetic config JAG seam; scene readiness fail-closed without maps
- Offline replay logout + action + inv-full + varp/widget/reset path

BLOCKED / external (not claimed):

- Authoritative 289 game-cache pairing, checksums, and real asset/render proof
- Approved live endpoint, credentials, test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Tutorial / guardian / random-event policy (explicitly not impl this milestone)
- Social/event outbound rows beyond the mapped table remain unused or
  fail-closed if an unmapped 274 constant is emitted on R289

No pushes, merges, remotes, submodule, host, live-server, or other-checkout work.
