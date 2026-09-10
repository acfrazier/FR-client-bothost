# Revision 289 cleanup A–H: aggregate integration review

Task: `t_82bfdbb6`  
Role: independent aggregate integration review (not a duplicate H section review)  
Model: grok-4.5  
Provider: xai-oauth  
Worker session: reviewer profile on this checkout  
Date (UTC): 2026-09-09

## Frozen code identity

| Field | Value |
| --- | --- |
| Branch | `codex/revision-289-client` (verified) |
| HEAD (frozen code under review) | `0406ceb56aea76e2d4474bbd54f8441f9b0f5245` |
| HEAD subject | `fix(client): complete revision 289 outbound input lifecycle` |
| Contract base (approved semantic audit/design) | `0b89fc3fc3e41edc096a1a252e4a1c837aff914d` |
| Workspace | `/Users/acfrazier/experiments/FR-client-289` |
| `CARGO_TARGET_DIR` | `/Users/acfrazier/experiments/FR-client-289/target` |
| Parent H approval | `t_34667a70` approved; report `docs/revision-289/cleanup-h-report.md` |
| Diff span `0b89fc3..0406ceb` | 17 commits, 33 paths, +7803/−316 (implementation + section reports + tests) |

Untracked non-deliverables present and left unstaged (inherited):  
`docs/revision-289/branch-final-review.md`, five `tools/*` tracers.  
This review commits **only** this report.

## Audited scope

Primary contract documents:

- `docs/revision-289/full-dispatch-cleanup-plan.md` decisions 1–8 and sections A–H
- `docs/revision-289/full-dispatch-audit.json` schema 2 (70 real inbound ops; 82 outbound)
- Section reports A–H and `offline-fixture-isolation-report.md`
- `docs/revision-289/STATE.md`, local `AGENTS.md`

Production surface reviewed as an aggregate (not last-commit-only):

- Inbound lifecycle: `R289Operation` decode → packet-free apply → `R289Outcome` / `R289Publication` in `client.rs`, with private modules `actor_289.rs`, `zone_289.rs`, `misc_289.rs`
- Frame admission: fixed-size exact match (including zero), stamped `frame_end`, string terminator-in-frame, inventory zero without allocation fallback, actor bit/byte bounds, enclosed whole-frame preflight
- Outbound: eight input/lifecycle emitters at real owners; ordered-byte coverage for all 82 rows; authorized mouse whole-sample budget; empty 232; packed click; camera pitch-then-yaw
- Preservation: R274 default construction / public constants / bump_gens R274-only; CPU/GPU ownership; `scene_state==1` last-FBO freeze; no bot action API

This is offline protocol/lifecycle/integration review. It does **not** claim live server/cache pairing, RSA/ISAAC encrypted-login correspondence, real presentation/audio, or whole-client acceptance.

## Method

Round-1 aggregate lens: cold read of production structure and section contracts, then comparison to handoff claims, then focused independent re-execution. Full 950-test all-features suite was **not** re-run here; H/root already retained a direct receipt at this HEAD (`target/cleanup-h-all-features-final.json`, returncode 0, 188.57s, 72 ok summaries / 950 passed). Prior failed full-suite exit 101 remains preserved at `target/cleanup-h-workspace.log` (stage1 welcome/logout readiness race before the authorized test-only feed repair).

## Section closure map (contract → production)

| Section | Owned inbound IDs | Production seam | Independent section/suite evidence used |
| --- | --- | --- | --- |
| A | 120, 121, 253 | `R289Operation` UpdatePid/Logout/LastLoginInfo; method110 welcome; Reset vs Applied | stage1 fixed/framing/welcome/logout/transport tests |
| B | 21 interface rows (12…252 per plan) | `R289InterfaceOperation` + per-client overlay; 65535 tab clear; transmog head; single-modal anim reset | stage2 cleanup_b fixtures |
| C | 28, 46, 75, 76, 97, 107, 154, 172, 195 | staged inv/varp/stat ops; u16/uSmart widths; stop-transmit IDs only; signed varps; tutorial chat redraw on change | stage2 cleanup_c fixtures |
| D | 65, 188, 201 | `actor_289` staged decode/apply; R289 hit +300; SAY/chat publication on actual log; reset-anims both families | `revision_289_actors` 13 |
| E | 15 zone/rebuild IDs incl. 91 area-synth | `zone_289` direct+enclosed; whole enclosed preflight; full-follows loc expiry; LOC_ANIM SE/0..103; rebuild freeze lifecycle | `revision_289_zones` 8 + cold-cache isolation |
| F | 13, 21, 47, 168, 196, 235, 243 | social ops; ignore ÷8/capacity; private ≥13; kind0 tutorial; chalreq body; notice gens only on change | stage2 cleanup_f fixtures |
| G | 12 camera/audio/hint/HUD IDs | `misc_289`; six-byte hints+padding; camera snap/clamp; audio gates; map_flag/multiway/reboot | `revision_289_misc` 10 |
| H | all 82 outbound | real owners for 8 missing emits; protocol/fixture corrections; ordered-byte table | `revision_289_outbound` 27 |

Plan partition accounts for all 70 primary inbound branch IDs; unsupported domain IDs (186, including length-6 ID 122) remain T1/logout, not invented handlers.

## Cross-cutting integration checks

### 1. Transaction boundaries / exact consumption / non-publication on failure

- `handle_packet` R289 path: `catch_unwind` around `dispatch_packet_289`; success publishes returned set once; T2/logout reset without success-family bump when reset marker set.
- Decode builds validated ops before apply; misc/zone/actor modules assert frame bounds; non-misc decode ends with `assert_eq!(payload.available(), 0, …)` (message still says “Section A” for later sections — naming debt only).
- Enclosed zones validate all inner IDs/lengths before origin mutation or prefix effects (zone suite unknown/short/invalid-tail cases).
- Malformed actor masks do not apply movement/chat prefixes (actor truncated combination tests).
- Fixed short/long admission rejects before field mutation (G every-fixed-frame test; A fixed admission).

### 2. Generation domains

- `R289Publication` is the sole R289 success publication path; `bump_gens` asserts R274-only.
- Families observed: inv/varp/stat/iface/chat/scene/camera/map_flag/world/player/npc; rebuild/logout all; reset-anims player+npc; loc-merge scene+player; chat only when log/notice actually changes where required.
- Hints/minimap/audio/reboot remain direct state without invented public families (matches decision 7).

### 3. Widget / actor / zone / social / camera / audio coupling

- Interface overlays stay per-client `Arc` mutation; no shared cache def writes from packet apply.
- Zone rebuild retains async map lifecycle and last-FBO freeze ownership in existing body; E rebuild test keeps world until load.
- Area synth 91 is zone-owned; global synth 177 is G-owned (no ID confusion).
- Tutorial kind0 storage (F) couples with B tutorial component/flash paths without moving draw ownership.
- Camera/audio request queues stay on existing on-demand owners; synthetic pixel/request fixtures are not live presentation proof.

### 4. Outbound input-loop emit ownership (H)

Confirmed structure and suite coverage for:

| Emit | Owner site (production) |
| --- | --- |
| mouse move/click, focus, camera | after inbound poll in game loop / `input_packets_289` before click consume |
| cycle7 | after drag handling |
| idle | after input/simulation, before keepalive |
| cycle4 | `handle_chat_input` entry |
| cycle5 | draw mode2 crosshair path only |
| recorder | scoped 50ms `Client::run` thread; join on Drop |

R289 pointer zero + separate click latch; R274 press/blur defaults preserved via revision/telemetry branching (outbound + input suites).

### 5. Permitted design decisions vs accidental Java divergence

Explicitly treated as **permitted / authorized**, not accidental bugs:

1. **Mouse move framing:** forward `payload < 240` before each whole sample; max emitted payload 243; ordered leftovers; not literal J:5829 reversed subtraction (root-authorized; goldens in outbound suite).
2. **Cyclelogic2:** deterministic legal choice sequence, not Java RNG distribution parity.
3. **R289 hit timer +300** vs preserved R274 +400.
4. **R289 LOC_ANIM** primary edge 0..103 and true SE height; R274 path unchanged.
5. **Empty cycle7/232** length 0 (Java + isolated engine), correcting old length-1 table claim.
6. **Camera 193** pitch then yaw; **click 224** single packed p4.
7. **R289 pile valuation** wrapping arithmetic for large stacks (R274 unchanged).

No host API, foreign bot policy, or manufactured runtime was introduced for these.

### 6. R274 default / construction / errors

- Default construction remains R274; session revision explicit and adopt_from fail-closed across revisions (stage3 adopt tests in H full receipt).
- Shared chat-effect path is revision-gated (stage3 default_revision + R289 effect tests).
- Legacy server_packets / gens / logout suites still pass under focused re-run.

### 7. Cache / renderer boundary / last-FBO freeze

- Packet handlers do not upload meshes or allocate renderer textures as ownership transfer.
- `freeze_last_scene(FrameKind::Game, scene_state==1)` still present; lib test `freeze_last_scene_only_while_the_game_is_loading` passed on this review run.
- GPU chrome dirty / scene test lock from earlier fc5516c milestone remain outside A–H delta ownership but are part of frozen HEAD behavior covered by H’s 950-test receipt.

### 8. Ledger completeness vs production semantics

Contract verifier PASS 256/82/50 is structural inventory only. Semantic confidence rests on production decode/apply paths plus independent production-path fixtures (actors/zones/misc/outbound/stage1–3), not on JSON row presence alone.

## Independent verification executed this review

All commands: cwd `/Users/acfrazier/experiments/FR-client-289`,  
`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`, serial, no overlapping Cargo.

| Command | Result |
| --- | --- |
| `python3 tools/verify_revision_289_contract.py` | PASS 256 inbound / 82 outbound / 50 fixtures; exit 0 |
| `cargo test -p client --test revision_289_stage1 --test revision_289_stage2 --test revision_289_stage3 --test revision_289_actors --test revision_289_zones --test revision_289_misc --test revision_289_outbound --test input -- --test-threads=1` | exit 0; counts **47 / 48 / 23 / 13 / 8 / 10 / 27 / 16** (log `target/cleanup-aggregate-focused.log`) |
| `cargo test -p client --lib freeze_last_scene -- --test-threads=1` | 1 passed |
| `cargo test -p client --test gens --test logout --test packet --test server_packets -- --test-threads=1` | gens/logout/packet/server_packets green (server_packets 19/19) |
| `git diff --check 0b89fc3..0406ceb` | clean |
| Recount of `target/cleanup-h-all-features-final.json` + log | returncode 0; 72 `test result: ok` summaries; **950** passed sum; **0** FAILED summaries |
| Preserve prior failure | `target/cleanup-h-workspace.log` still shows stage1 `welcome_then_logout… FAILED` (45 passed / 1 failed) under exit 101 era |

No retry-until-green; no assertion weakening; no source implementation edits in this review.

## Actionable findings

### Blocking (must fix before offline aggregate acceptance)

**None identified** against the A–H cleanup contract at HEAD `0406ceb`.

### Non-blocking / maintenance (root may fix opportunistically; do not block offline A–H acceptance)

1. **Dead dual-path residual in `dispatch_packet_289`** (`client.rs` ~7462–7596): after successful `R289Operation::decode`, a legacy match still contains arms for CHAT_FILTER_SETTINGS, IF_CLOSE, IGNORELIST, FRIENDLIST_LOADED, MESSAGE_GAME, CAM_RESET, MINIMAP_TOGGLE, SET_PLAYER_OP, IF_SETTAB, IF_SETTEXT, IF_SETANIM, IF_SETCOLOUR, IF_OPENMAIN_SIDE, IF_OPENSIDE, IF_OPENOVERLAY. These arms are unreachable for IDs that decode returns `Some` for, but they omit modern publication/sentinel behavior (e.g. IF_SETTAB without 65535→-1). Residual risk is future decode breakage silently reactivating weaker semantics. Prefer deleting unreachable arms once root confirms no intentional fallback.
2. **Assert message debt:** non-misc exact-end assert still labeled `"unconsumed Section A frame"` while covering B–F ops.
3. **STATE.md narrative lag:** still describes H as not-yet-accepted and “Next: commit H…” though HEAD is already `0406ceb` and H was approved on `t_34667a70`. Update after this aggregate card.
4. **protocol-289.json empty-payload style:** length-0 opcode 232 may use `fields=[]` while peers use `["(empty payload)"]`; verifier allows both (noted on H review). Cosmetic contract consistency only.
5. **Historical honesty limits (still external to A–H closure claims):** packed-chat body depth and some appearance/type-binding edges remain fixture-bounded; widget strings rely on frame stamp + terminator rather than a separate absolute payload-byte cap beyond the frame; synthetic audio/GPU fixtures ≠ audible/live presentation. Carry forward into whole-branch Grok4.6, not as A–H reopeners unless new counter-evidence appears.

### Out of scope / remaining root gates (must stay visible)

- Required **separate** whole-branch Grok4.6 review after root resolves any findings it chooses to act on
- Authentic 289 game-cache pairing, checksums, real asset/render proof
- Approved live endpoint / credentials / test-account authorization
- Live RSA/ISAAC modulus/endpoint compatibility (offline ISAAC +50 is seed-transform only)
- Live login/action/logout and real presentation
- No merge/push/remote/host work in this review

## Explicit offline verdict

**OFFLINE ACCEPTED (bounded)** for the combined revision-289 dispatch cleanup sections **A–H** at frozen HEAD **`0406ceb56aea76e2d4474bbd54f8441f9b0f5245`**, relative to approved design base **`0b89fc3`**.

Meaning:

- The aggregate receive → frame → apply → publication → lifecycle design is implemented for all 70 named inbound operations and all 82 outbound rows with production semantics backed by meaningful independent tests, not ledger presence alone.
- Cross-section coupling (generations, zones/actors/social/camera, outbound ownership, R274 preserve, last-FBO freeze) holds under focused re-verification and the retained full-suite receipt.
- Authorized design exceptions (mouse whole-sample budget, deterministic cycle2, revision-gated timers/heights) are distinguished from accidental Java divergence.
- This is **not** live acceptance, **not** whole-client acceptance, and **not** a substitute for the required separate Grok4.6 whole-branch review.

## Reviewer checks (checklist)

- [x] Branch and exact HEAD/base commits verified
- [x] Plan decisions 1–8 and sections A–H read
- [x] Audit schema-2 inventory used as operation ledger
- [x] Section reports A–H + fixture-isolation report read
- [x] Production decode/apply/publication/outbound structure inspected across modules
- [x] Dead residual dispatch arms identified (non-blocking)
- [x] Focused suites re-run with recorded exits
- [x] Contract verifier re-run
- [x] H full-suite receipt recounted; prior exit 101 preserved
- [x] freeze_last_scene lib test re-run
- [x] No source implementation edits; report-only commit intended
- [x] Offline verdict does not claim live/whole-client acceptance
