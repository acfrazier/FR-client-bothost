# Cleanup D: staged actor updates

Task t_115f2843; foundation 4a2f5aa; branch codex/revision-289-client.
Implementation pending same-card Grok4.5 review, not accepted/live proof.

## Scope and source

Only IDs 65 (NPC_INFO), 188 (PLAYER_INFO), 201 (RESET_ANIMS).
Primary RuneWiki/openrs2-nonfree revision289 pin
0c00ef249546fada67b1f6eb8bbe01ea7c250c95, as recorded in
full-dispatch-audit.json. Read-only source root:
/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/.

Source anchors in client.java:
- PLAYER_INFO dispatch 2819-2823; method139 orchestration; method212 local
  movement, method185 old players, method172 extended masks, method153 new
  players (10-bit loop threshold), method128:5113-5259 ordered masks.
- NPC_INFO dispatch 3380-3383; method187 orchestration; method226 old NPCs,
  method124 new NPCs (21-bit loop threshold), method222 ordered masks.
- RESET_ANIMS dispatch 3496-3508.
- Class1_Sub1_Sub1_Sub1_Sub1.method39:92 onward: appearance, transform part
  loop termination, colours, seven animations, name/combat/skill and hash.

Fixtures are independently constructed bits/bytes from these public primary
contracts, not copied captured payloads or private server material. Small
synthetic cache definitions distinguish animation priorities/duplicate modes
and NPC metadata; they do not claim authentic cache compatibility.

## Implementation contract

actor_289.rs is a private module under client.rs. R289Operation::decode borrows
Client immutably, stages movement/mask values and validated appearance data,
checks declared bit/byte bounds and complete frame consumption, then returns
an ActorFrame. Apply reads no packet and mutates existing actor allocations;
there is no Client/World clone, rollback or side-effectful second parse.
Cached appearance bytes are decoded into values before apply too.

Local actor ownership remains local_player, not players[2047]. New actors use
the post-local-movement origin. Retained/removal/update lists and cycle-based
remove/re-add identity follow primary order. Mask order is source order, not
legacy symbolic hit names or mask-value heuristics. NPC metadata binding retains
the existing Rust left/right naming convention. Appearance keeps unwritten
parts on transforms; animation priority/duplicate reset, spot sentinel/signed
height/delay, exact movement times and route abort are preserved.

R289 hit bars use cycle+300; R274 remains on its unchanged +400 decoder.
Player SAY strips one leading tilde and logs local or tilde messages; ordinary
remote SAY stays overhead-only. Public chat retains name/ready/ignore/staff/
disabled gates. Apply returns chat publication only when add_chat runs; outer
publication coalesces even multiple messages into one generation increment.
RESET_ANIMS returns both actor families once. Declared-zero actor frames never
read stale backing storage. Malformed prefixes reach existing T2/lifecycle
reset without applying movement, masks, appearance or chat prefixes.

## Production-path fixtures

crates/client/tests/revision_289_actors.rs has 13 tests, including loops covering:
- local walk/run/teleport, old retention/mask/walk/run/remove/count shrink;
- new signed offsets, sentinel/no-new endings, metadata, remove/re-add identity,
  cached appearance and post-local-movement origin;
- each player mask 1/2/4/8/16/32/64/256/512/1024 plus all-mask combination;
- each NPC mask 1/2/4/8/16/32/64/128 plus all-mask combination;
- transformed appearance; priority, duplicate reset and spot sentinel;
- exactmove coordinates/times/route abort; all hit slots and health timers;
- local/remote/tilde SAY; public-chat staff 0/1/2/3/4 across all gates;
- truncated combined masks, later invalid appearance, declared bit bounds and
  unchanged prefix state with only lifecycle generation invalidation;
- explicit default-R274 hit timers and SAY behavior.

revision_289_stage2 reset_anims_clears_primary now checks both generations,
unrelated generations, successful session state and empty-frame consumption.
Four old R289 stage2 hit timer expectations were corrected from +400 to +300,
justified by method128:5177/5251 and method222, not weakened to hide failures.

## Executed verification

All commands ran in the client checkout with
CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target.

- cargo test -p client --test revision_289_stage2 reset_anims_clears_primary -- --test-threads=1:
  baseline passed, added generation regression failed, implementation passed.
- cargo test -p client --test revision_289_actors -- --test-threads=1:
  initial regressions failed before implementation; final expanded suite 13/13.
  An intermediate invalid no-sentinel player-mask fixture failed at bit bounds;
  corrected its sentinel per method153, without weakening production bounds.
- cargo test --workspace --no-fail-fast -- --test-threads=1:
  first run failed: stage2 two tests exposed four obsolete +400 assertions;
  stage1 fixed_empty_sync_after_welcome_and_unknown_frame_reset_once also failed
  (already recorded intermittent in accepted C review). Preserved output:
  target/cleanup-d-workspace.log. No unrelated production fix was made.
- cargo test --workspace --all-features --no-fail-fast -- --test-threads=1:
  PASS, including client lib76, actors13, stage1 46, stage2 42, stage3 23,
  server_packets19, gens12, logout7, player_info1, GPU suites and client-play4.
  Full output: target/cleanup-d-all-features.log. Stage1 passed in this run;
  that does not erase the prior intermittent failure.
- cargo check --workspace --all-features: PASS (client and client-play).
- rustfmt --edition 2021 --check crates/client/src/client/actor_289.rs crates/client/tests/revision_289_actors.rs: PASS.
  Existing unrelated formatting in client.rs/stage2 was not reformatted.
- python3 tools/verify_revision_289_contract.py: PASS 256 inbound / 82 outbound /
  50 fixtures. This verifies the existing contract, not full live semantics.
- git diff --check: PASS.

## Remaining gates

Same-card reviewer=reviewer is next. Sections E-H are untouched and remain
root-dispatched. Authentic cache/server pairing, approved live endpoint/account,
RSA/ISAAC/presentation proof and final whole-branch Grok4.6 remain separate root
gates. No host API, renderer ownership, scene_state1 freeze, remote, push,
server/cache source or live-host changes. Existing untracked campaign artifacts
are deliberately excluded from this commit.
