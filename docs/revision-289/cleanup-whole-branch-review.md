# Revision 289 cleanup: required Grok 4.6 whole-branch review

Task: `t_d1a06f94`
Role: required final `branchreviewer` (not a section review, not release authorization)
Model: grok-4.6
Provider: xai-oauth (profile defaults; no task model/provider override)
Worker session: `20260908_221302_f73107`
Date (local): 2026-09-08 EDT

## Verdict: OFFLINE ACCEPTED (bounded)

Independent Grok 4.6 whole-branch review of `codex/revision-289-client` after
cleanup A–H and the aggregate integration review. This is **not** whole-client
acceptance, **not** live login/action/logout proof, **not** authentic cache
pairing, and **not** audible/rendered presentation proof.

Predecessor receipts are **preserved**, not rewritten:

- `t_77d35bfb` REJECTED `0030afb` — `docs/revision-289/branch-review.md`
- `t_615aa9ac` OFFLINE ACCEPTED (bounded) `0e3b6a7` — `docs/revision-289/branch-rereview.md`
- `t_f4e2ad64` OFFLINE ACCEPTED (bounded) `fc5516c` — `docs/revision-289/branch-final-review.md` (untracked in this tree; identity kept)
- Cleanup H `t_34667a70` approved (Grok 4.5 / xai `20260908_220159_daf89a`)
- Aggregate `t_82bfdbb6` OFFLINE ACCEPTED (bounded) code `0406ceb` (Grok 4.5 / xai `20260908_220659_2e28d6`)

Those documents are evidence. They are not a waiver to skip code.

## Frozen identity

| Field | Value |
| --- | --- |
| Branch | `codex/revision-289-client` (verified) |
| Original R274 published base | `4f2048ea10f75b3bb92ff45610b35ba7313b0308` |
| Production code tip | `0406ceb56aea76e2d4474bbd54f8441f9b0f5245` (`fix(client): complete revision 289 outbound input lifecycle`) |
| Aggregate report commit | `e4834d78150a095311013aad9e77b239020af916` |
| Reviewed HEAD | `4bc40d8a938935be23dc4af6ca64b9f48a222f2f` (`docs: record cleanup aggregate acceptance before final review`) |
| Design/audit base (cleanup contract) | `0b89fc3fc3e41edc096a1a252e4a1c837aff914d` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| `CARGO_TARGET_DIR` | `/Users/acfrazier/experiments/FR-client-289/target` |
| Implementation edits by this reviewer | none |
| Follow-up cards created | none |

Code freeze check (this run):

- `git rev-parse` of the four named commits matches the full hashes above.
- `git diff --stat 0406ceb..HEAD` is only `docs/revision-289/STATE.md` and `docs/revision-289/cleanup-aggregate-review.md`.
- `git diff --name-only 0406ceb HEAD -- ':!docs' ':!*.md'` is empty. Reviewed production code is unchanged after `0406ceb`.
- Working tree has no tracked modifications. Inherited untracked non-deliverables left unstaged: `docs/revision-289/branch-final-review.md` and five `tools/*` tracers.

Span `4f2048e..0406ceb`: 44 commits, 64 files, +21351/−385.

## Scope read

- Local `AGENTS.md`, `docs/revision-289/STATE.md`, `plan.md`, `full-dispatch-cleanup-plan.md` decisions 1–8, `full-dispatch-audit.json` schema 2 (70 inbound / 82 outbound), aggregate review, section reports A–H, fixture-isolation report, prior branch-review / re-review / final-review receipts.
- Production: `R289Operation::decode` → packet-free `apply_operation_289` → `R289Outcome` / `R289Publication` in `crates/client/src/client/client.rs`, plus `actor_289.rs`, `zone_289.rs`, `misc_289.rs`, `outbound_289.rs`, `mouse_recorder_289.rs`, `crates/client/src/io/revision.rs`, `client_prot_289.rs`, `gpu.rs` freeze helper, `draw.rs` cycle5/multiway.
- Tests inspected as behavior oracles (truncated masks, enclosed unknown tails, mouse whole-sample budget, ordered outbound bytes), not ledger presence.

Lens: artifact-first cold read of decode/apply/publication/outbound, then comparison to handoff claims, then focused independent execution. Full 950-test all-features suite was **not** repeated; H receipt was recounted and focused suites were re-run.

## Contract map (70 inbound / 82 outbound)

Audit inbound unique IDs (70): 12,13,18,21,23,28,29,30,35,46,47,55,59,60,63,65,71,73,75,76,79,81,82,83,87,90,91,97,106,107,112,115,117,119,120,121,127,133,136,138,144,154,155,160,164,168,172,176,177,181,184,187,188,189,194,195,196,201,204,208,211,219,222,233,235,243,244,247,252,253.

Unsupported domain IDs (186, including length-6 ID 122) are not invented handlers. `dispatch_packet_289` `_` arm is T1 + `logout` (`client.rs` 7597–7603) after decode returns `None`.

Every real inbound ID has a production decode path before apply:

| Owner | IDs | Seam |
| --- | --- | --- |
| A lifecycle | 120, 121, 253 | `UpdatePid` / `Logout` / `LastLoginInfo`; welcome 650/655 + method110 close ack (`client.rs` 4851–4900) |
| B interfaces | 12,18,23,30,35,55,59,63,79,81,119,127,138,160,181,184,189,211,222,244,252 | `R289InterfaceOperation`; tab 65535→-1 at decode (`client.rs` 497–498); transmog head (`4826–4838`); single-modal `if_anim_reset` vs combined open without it (`4840–4843`) |
| C inv/varp/stat | 28,46,75,76,97,107,154,172,195 | u16 full / uSmart partial; zero-frame stamp in `dispatch_packet_289` (`7431–7447`); stop-transmit IDs only; signed varp/weight; tutorial chat redraw on changed varp (`4720–4728`); sync without tutorial chat redraw (`4732–4745`) |
| D actors | 65, 188, 201 | `actor_289` staged bits/masks; R289 hit `cycle + 300` (`actor_289.rs` 617); R274 `+ 400` remains on shared helpers (`client.rs` 7974, 8104, 8319, 8390); SAY/chat publish on actual log; reset-anims player+npc (`4518–4532`) |
| E zones | 60,71,83,87,90,91,106,112,117,144,155,176,194,219,233 | `zone_289` direct+enclosed; unknown inner panics (`zone_289.rs` 290) so apply never runs; area-synth 91 distinct from global synth 177; full-follows 144 loc expiry; LOC_ANIM `0..103` and true SE height (`382`, `412`); rebuild publishes `ALL` and keeps world until load |
| F social | 13,21,47,168,196,235,243 | ignore ÷8 and capacity 100 (`client.rs` 455–457); private ≥13 (`462–463`); chalreq body (`4647–4664`); kind0 tutorial storage (`4668–4671`); chat gens only when log/notice changes |
| G misc | 29,73,82,115,133,136,164,177,187,204,208,247 | `misc_289::decode` numeric match; six-byte hints with padding (`misc_289.rs` 123–153`); camera snap/clamp/shake; audio gates; no new public gens for hints/minimap/audio/reboot |

H outbound: all 82 named rows in `protocol-289.json` / `ClientProt289`. Eight previously missing emitters exist at real owners:

| Emit | Owner |
| --- | --- |
| mouse move/click, focus, camera pitch-then-yaw | `input_packets_289` after inbound poll (`client.rs` 11048–11049; `outbound_289.rs` 83–116) |
| cycle7 / empty 232 | after drag, before walking (`client.rs` 11091–11097) |
| idle | after input/simulation (`client.rs` 11147–11151) |
| cycle4 | `handle_chat_input` entry (`client.rs` 9314–9320) |
| cycle5 | draw mode-2 crosshair only (`draw.rs` 509–516) |
| recorder | `Client::run` scoped 50 ms thread (`client.rs` 11531; `game_shell.rs` `start_mouse_recorder`) |

R289 pointer zero + separate click latch (`construct` `client.rs` 1277–1281). R274 press/blur defaults preserved via revision/telemetry branching.

Op-table completeness and fixture execution are **not** treated as live proof.

## Cross-module checks

### Frame validation before application/publication

- Fixed sizes including zero: `assert_eq!(end, size)` when table size ≥ 0 (`client.rs` 7450–7456).
- `gjstr` requires newline inside `frame_end` (`packet.rs` 244–257).
- Actor bits/bytes bounded against `frame_end` (`actor_289.rs` 93–113). Truncated combination tests prove movement/chat prefixes are not applied.
- Enclosed zones collect/validate inner ops, then apply origin/effects (`zone_289.rs` 90–148). Unknown/short/invalid-tail tests reject before sound prefix.
- `handle_packet` publishes returned set once on `Applied`; `Reset` skips extra success-family bump (`4034–4048`). `bump_gens` asserts R274-only (`4071–4075`). Logout still invalidates all through `logout` → `bump_all_gens` (`9031`).

No malformed-prefix publication path was found on the R289 decode/apply seam.

### R274 preserve / GPU-CPU / last-FBO

- Default construction `Client::new` → `ClientRevision::R274` (`client.rs` 1167; `revision.rs` 12–16). Public `ServerProt` / `ClientProt` / `SERVER_PROT_SIZES` remain the 274 tables.
- MESSAGE_PUBLIC effects revision-gated (`client.rs` 9489–9520): default sequential wave:=1 / scroll:=2; R289 else-if wave2/wave/shake/scroll/slide → 2/1/3/4/5.
- `freeze_last_scene` is still `kind == FrameKind::Game && scene_state == 1` (`gpu.rs` 1599–1600). `git diff 4f2048e 0406ceb -- gpu.rs` has no freeze/minimap_live/minimap_held hunks; the +20/−1 is the earlier fc5516c chrome dirty-gate + test-only scene lock.
- Packet apply does not take renderer mesh/texture ownership. No host bot action API (`lib.rs` 3).

### Authorized decisions (not accidental Java bugs)

1. Mouse whole-sample framing: forward `payload < 240` before each sample (`outbound_289.rs` 48–50); a 4-byte write at 239 yields max payload 243; leftovers retained. Root-authorized vs reversed J:5829 subtraction.
2. Cyclelogic2 deterministic legal sequence (`client.rs` 3517–3535), not Java RNG distribution.
3. R289 hit timer +300 vs preserved R274 +400.
4. R289 LOC_ANIM edge 0..103 and true SE height; R274 path unchanged.
5. Empty cycle7/232 length 0 (`client_prot_289.rs` CYCLELOGIC7 length 0; `protocol-289.json` id 232 length 0).
6. Camera 193 pitch then yaw; click 224 packed p4.
7. R289 pile valuation wrapping (`client.rs` 10561) with R274 unchanged.

ISAAC `seed.wrapping_add(50)` before `random_in` (`client.rs` 2433–2437) is an offline seed transform only. It is not encrypted-login or live-modulus correspondence.

## Independent verification (this run)

cwd `/Users/acfrazier/experiments/FR-client-289`, `CARGO_TARGET_DIR` this checkout, serial, no overlapping Cargo, no retry-until-green, no assertion edits.

| Command | Result |
| --- | --- |
| `python3 tools/verify_revision_289_contract.py` | PASS 256 inbound / 82 outbound / 50 fixtures |
| `cargo test -p client --test revision_289_stage1 --test revision_289_stage2 --test revision_289_stage3 --test revision_289_actors --test revision_289_zones --test revision_289_misc --test revision_289_outbound --test input -- --test-threads=1` | exit 0; **47 / 48 / 23 / 13 / 8 / 10 / 27 / 16** |
| `cargo test -p client --lib freeze_last_scene -- --test-threads=1` | 1 passed |
| `cargo test -p client --test gens --test logout --test packet --test server_packets -- --test-threads=1` | gens 12, logout 7, packet 7, server_packets 19 |
| Recount `target/cleanup-h-all-features-final.json` + `.log` | returncode 0; 188.57s; 72 `test result: ok`; **950** passed; 0 FAILED summaries |
| Preserve prior failure | `target/cleanup-h-workspace.log` still contains `welcome_then_logout` FAILED (exit-101 era; test-only receiver repair in `0406ceb`) |

`git diff --check 4f2048e..0406ceb` reports trailing whitespace in historical `docs/revision-289/gpu-workspace-gate-diagnosis.md` only. Not production code. Aggregate's `0b89fc3..0406ceb` check remaining as previously recorded.

Meaningful test omissions (honesty, not blockers of this offline verdict): packed-chat body / appearance type-binding remain fixture-bounded; widget strings use frame stamp + terminator rather than a separate absolute payload-byte cap; synthetic audio/GPU fixtures are not live presentation; `all_82_declared_lengths_match_primary` is structural and is **backed** by ordered-byte emit tests through UI/action/input owners, not used alone.

## Findings

### Blocking (must fix before this offline whole-branch accept)

**None identified** against the required cleanup contract at frozen production `0406ceb` / HEAD `4bc40d8`.

### Non-blocking (root may defer; independently assessed)

1. **Dead dual-path arms in `dispatch_packet_289`** (`client.rs` 7458–7604). After `R289Operation::decode` returns `Some`, apply returns immediately. The following match still contains CHAT_FILTER_SETTINGS, IF_CLOSE, IGNORELIST, FRIENDLIST_LOADED, MESSAGE_GAME, CAM_RESET, MINIMAP_TOGGLE, SET_PLAYER_OP, IF_SETTAB (no 65535→-1), IF_SETTEXT, IF_SETANIM, IF_SETCOLOUR, IF_OPENMAIN_SIDE, IF_OPENSIDE, IF_OPENOVERLAY. Those IDs all decode today (misc 133/136; the rest in `R289Operation::decode`). **Currently unreachable, therefore currently safe.** Residual risk is a future decode `None` silently reactivating weaker semantics. Prefer deleting the arms; do not treat them as a live dual dispatcher.
2. **Assert label debt:** non-misc exact-end still says `"unconsumed Section A frame"` (`client.rs` 571) while covering B–F.
3. **`protocol-289.json` opcode 232** uses `fields: []` while some length-0 peers use `["(empty payload)"]`. Verifier allows both. Cosmetic.
4. **Historical docs whitespace** in `gpu-workspace-gate-diagnosis.md` under whole-branch `git diff --check`. Not a protocol defect.
5. **Stale comments** (e.g. `add_chat` still says tutorial message is not ported at `client.rs` 4904–4906 while F stores kind0; `ServerProt289` header still talks about untraced IDs). Comment debt only.

### Remaining root gates (must stay visible)

- Authentic 289 game-cache pairing, checksums, real asset/render proof
- Approved live endpoint / credentials / test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility
- Live login / action / logout and real presentation
- Tutorial / guardian / random-event policy (explicitly out of this milestone)
- Repo hygiene (merge/push/remotes) remains parent Codex

Offline acceptance of this branch does **not** establish those.

## Explicit offline verdict

**OFFLINE ACCEPTED (bounded)** for the whole `codex/revision-289-client` implementation relative to original published base **`4f2048ea10f75b3bb92ff45610b35ba7313b0308`**, with production code frozen at **`0406ceb56aea76e2d4474bbd54f8441f9b0f5245`** and docs-only HEAD **`4bc40d8a938935be23dc4af6ca64b9f48a222f2f`**.

Meaning:

- All 70 primary inbound operations and all 82 named outbound operations map to actual production decode/apply/emit paths with exact-frame validation before publication.
- Revision selection, session adopt, framing, login wrapper `p2` revision, ISAAC seed +50, cache/config bind, rendering ownership, and last-FBO freeze are consistent with the campaign contract.
- Authorized mouse/cycle2/hit/loc/valuation decisions are retained as decisions, not accidental divergence.
- Dead fallback arms are maintenance debt, independently judged currently safe because unreachable.
- This does **not** authorize merge, live proof, host integration, or whole-client acceptance.

## Reviewer checks

- [x] Exact full hashes verified; production code unchanged after `0406ceb`
- [x] Plan decisions 1–8, audit schema 2, A–H + aggregate + prior branch receipts read as evidence
- [x] All 70 inbound IDs mapped to production decode/apply
- [x] All 82 outbound rows accounted; eight H emitters inspected at owners
- [x] Lifecycle/generation/owner/cross-module paths inspected
- [x] R274 defaults, public constants, error/T1/T2, last-FBO, GPU/CPU ownership
- [x] Dead fallback arms independently assessed (currently safe / non-blocking)
- [x] Focused suites re-run; H 950-test receipt recounted; prior exit 101 preserved
- [x] No source implementation edits; this file only
- [x] Offline verdict does not claim live/whole-client acceptance
