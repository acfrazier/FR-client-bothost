# Stage 3 corrective independent review (t_5f3a92c0)

## Verdict: REJECTED

Independent Grok 4.5 full-stage3 corrective review of commits
`f766c9aa53934cb19b5ed823a1fe47d131a04419`..`a00a64100bfb5d22e5affebc44641bf3770a896c`
on branch `codex/revision-289-client`.

| Field | Value |
| --- | --- |
| Reviewer task | t_5f3a92c0 |
| Model / provider | grok-4.5 / xai-oauth (profile reviewer defaults) |
| Reviewed HEAD | a00a64100bfb5d22e5affebc44641bf3770a896c |
| Stage3 range | f766c9a (accepted strict-inv/session) → d441614 (stage3 feat, rejected run496) → a00a641 (stage3 fix) |
| Prior receipts preserved | d441614 / run496 changes_requested (not acceptance); t_448637e1 run496 reject + round-2 execution approve on a00a641 is execution-lens only and does not replace this corrective full-table audit |
| Implementation edits by reviewer | none |
| Correction child | see Created correction card below |

This review is the required independent full-stage3 Grok4.5 verdict after
orchestration t_95bef768 omitted top-level reviewer on stage3 and run495
dispatched as implementer. Parent t_448637e1 execution-lens approve is retained
as evidence of rework closure for the two run496 defects, not as substitute
acceptance of the cold outbound/cache field-oracle requirements below.

## Scope inspected

- `docs/revision-289/AGENTS.md` via checkout `AGENTS.md`, `STATE.md`,
  `source-contract.md`, `protocol-289.json`, `plan.md`
- Diff stat f766c9a..HEAD: `client.rs`, `cache_289.rs`, `client_prot_289.rs`,
  `draw.rs`, `revision_289_stage3.rs`, `protocol-289.json`, `source-contract.md`,
  `manifest.json`, `tools/promote_outbound_289.py`
- Primary Java (read-only):
  `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/client.java`
  (openrs2-nonfree pin 0c00ef24 per source-contract)
- Production emit sites in `client.rs` + `draw.rs`; `Client::load_cache` /
  `new_with_revision`; `JagFile::new` header layout

## Independent native tests

`CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target`

| Command | Result |
| --- | --- |
| `cargo test -p client --test revision_289_stage3` | 20 passed |
| `cargo test -p client --lib` | 70 passed (includes io::cache_289) |
| `cargo test -p client --test revision_289_stage1` | 26 passed |
| `cargo test -p client --test revision_289_stage2` | 28 passed |
| `cargo test -p client --test do_action` | 13 passed |
| `cargo test -p client --test walk` | 6 passed |
| `cargo test -p client --test prot` | 2 passed |
| `cargo test -p client --test logout` | 7 passed |
| `cargo test -p client --test from_shared` | 6 passed |
| `cargo test -p client --test player_info` | 1 passed |
| `cargo test -p client --test login_rsa` | 2 passed |
| `cargo test -p client --test gens` | 12 passed |
| `cargo test -p client --test server_packets` | 19 passed |
| `cargo test -p client --lib freeze_last_scene` | 1 passed |
| `cargo check -p client -p client-play` | ok |

All listed 274-path / prior-stage suites green. No host tests run. No live hosts.

## Cold question results

### 1. Outbound IDs / lengths / production remap — PASS (with residual field defects)

- `ClientProt289` + `map_client_prot` fail-closed on unmapped 274 constants.
- Production emit routes through `Client::client_opcode` at client interact sites
  and the four draw sites (`draw.rs` 712/943/3303/4185). No bare
  `p1_enc(ClientProt::*.id)` remains in draw.
- Stage3 tests prove R289 walk 234, OPNPC2 21, OPLOC1 10, OPHELD1 76,
  INV_BUTTON1 44, IF_BUTTON 86, CLOSE_MODAL 93 via CLOSE_BUTTON,
  RESUME_P_COUNT 180 via `handle_chat_input`, draw 146/255/130/125 vs 274
  94/188/12/52, and R274 public ids preserved on the same paths.
- `protocol-289.json` outbound: 82 production_enabled rows, 0 `"length": "unknown"`.
- Spot-checked Java anchors match production for:
  - method206 MOVE_GAMECLICK 234 / MINIMAP 236 / OPCLICK 67 (size/ctrl/abs/steps)
  - method160 OPLOC* p2(x+base) p2(z+base) p2(locId)
  - OPHELD/INV_BUTTON p2 obj, p2 slot, p2 com
  - CLOSE_MODAL 93 len0; RESUME_P_COUNT 180 + method470 p4
  - FRIENDLIST/IGNORE p8 via method472 (production `p8`)
  - minimap 14-byte tail after type-1 tryMove (Java method225; Rust `minimap_loop`)
  - IDK_SAVEDESIGN 27: gender + 7 kits + 5 colours

### 2. Chat / social / variable packets — FAIL (blocking)

Primary Java `client.java` MESSAGE_PUBLIC (method465(156)) effect prefixes:

| prefix | Java effect byte |
| --- | --- |
| (none) | 0 |
| `wave:` | 1 |
| `wave2:` | 2 |
| `shake:` | 3 |
| `scroll:` | 4 |
| `slide:` | 5 |

Production `Client::handle_chat_input` (`client.rs` ~8668–8676) only implements:

- `wave:` → 1
- `scroll:` → **2** (wrong; must be 4)
- omits `wave2:`, `shake:`, `slide:`

Opcode remap to 156 and size/colour/wordpack frame shape are correct, but the
**source-ordered effect field encoding is wrong** on a production_enabled,
production-emitted row. Stage3 explicitly required per-row payload encoding
anchors for chat packets, not opcode/length tables alone. Available primary
Java makes this a correctable implementation defect, not an external gate.

Additional outbound contract residuals (same correction card):

1. **SEND_SNAPSHOT (94)** in `protocol-289.json` lists fields as
   `"p2*5 snapshot fields"`. Primary Java (design/abuse accept ~1969–1972) is
   `method465(94)` + `method472` (p8 namehash) + `method466` (reason) +
   `method466` (mute flag) = length 10. Field oracle must match. Report-abuse
   client codes 601–612 are described in `client_button` comments as “slice 6”
   but are **not implemented** there; row is still `production_enabled: true`.
   Either implement the Java-shaped emit path or stop claiming production_enabled
   until emit exists with a test.
2. Friend/ignore anchors saying `method468` should cite **method472** (p8).
3. **EVENT_MOUSE_MOVE (229)** contract fields are only `"move samples"` while
   Java encodes timed delta/absolute samples (method467/method469/method470 +
   psize1). Production mouse-move path must keep sample encoding aligned; contract
   must list ordered encodings, not a gloss.

Fixed-length NPC/loc/obj/player/held/inv/IF/walk families checked above remain
source-aligned at production sites.

### 3. Cache / config seam — PASS (offline synthetic only)

- `synthetic_jag`: outer g3/g3 = payload length **excluding** six-byte header
  (`2 + 10*n + data_len`); `debug_assert_eq!(out.len(), 6 + payload_len)`; unit
  test `synthetic_jag_header_excludes_outer_six` and `JagFile::new` agree
  (unpacked==packed → parse from offset 6).
- `load_offline_config_seam` calls production `Cache::unpack` / `IfType::unpack`
  (not header-only).
- Stage3 `offline_config_loader_synthetic_fixture` constructs
  `Client::new_with_revision(..., R289)` with synthetic cache_dir and asserts
  flo/varp/idk/iface bind via production `load_cache` — **not** a standalone
  unused seam.
- `authentic_cache_present` stays false; scene_state does not advance to 2 on
  synthetic config alone. Authentic cache remains an external gate for real
  assets/render/scene proof only (correctly scoped).

### 4. Offline native replay — PASS (bounded)

Stage3 tests cross production `tcp_in` / handle_packet with:

- LOGOUT 121 → stream drop, then walk/NPC action emit, scene fail-closed
- VARP_SMALL 75 + IF_OPENSIDE 252 + RESET_ANIMS 201
- UPDATE_INV_FULL 107 (length-prefixed) then walk action
- Isaac opcode-only encoding on MOVE_GAMECLICK

Strict consumed-length / rejection paths for inv-full remain covered by stage1
suites (re-run green). R289 inbound unknown mapping still fail-closed via
`dispatch_packet_289` (stage2 contract). No synthetic `scene_state = 2` counted
as real scene-build proof.

### 5. Scope / hygiene — PASS

- No bot action API added inside client.
- Public 274 `ClientProt` constants unchanged (spot-checked MOVE_GAMECLICK 207,
  CYCLELOGIC6 188, etc.).
- client-play `cargo check` ok.
- No private server/vault material copied into fixtures (synthetic flo/varp/idk/
  interface only).
- Untracked `tools/*.py` tracers remain uncommitted optional provenance helpers
  (non-blocking).
- No pushes/merges/remotes/submodule/host/live work performed by this review.

## Run496 defects (must stay closed) — PASS

| Defect | Status at a00a641 |
| --- | --- |
| draw.rs bare ClientProt.id for CYCLELOGIC6/1/3 + TUT_CLICKSIDE | Closed: all four use `client.client_opcode`; `r289_draw_paths_emit_289_not_274_opcodes` |
| protocol-289.json outbound unknown lengths while emit live | Closed: 0 unknown; source-contract allows enabled traced rows |

## Blocking defects (correction required)

1. **MESSAGE_PUBLIC effect prefix encoding** in production `handle_chat_input`
   must match primary 289 Java (wave/wave2/shake/scroll/slide → 1/2/3/4/5).
   Add a stage3 (or focused) test that emits sample prefixes and asserts effect
   bytes; keep colour table and wordpack/size framing.
2. **SEND_SNAPSHOT contract + emit honesty**: replace `"p2*5"` with
   p8 + p1 reason + p1 mute; either wire report-abuse 601–612 production emit
   through `client_opcode(REPORT_ABUSE)` with a golden test, or clear
   `production_enabled` until that path exists.
3. **Tighten weak outbound field anchors** for EVENT_MOUSE_MOVE sample encodings
   and friend/ignore method472 citations so production_enabled rows carry
   source-ordered field lists, not glosses.
4. **Contract verifier FAIL** (orch reproduction at a00a641; prior stage3
   reviewer omitted this gate):
   `python3 tools/verify_revision_289_contract.py` →
   `FAIL: outbound fields must be a non-empty ordered list`.
   Root cause: zero-payload outbound rows use `"fields": []` (e.g. CLOSE_MODAL,
   NO_TIMEOUT, MAP_BUILD_COMPLETE, IDLE_TIMER, ANTICHEAT_CYCLELOGIC5). Repair
   schema consistency **without weakening** the ordered-field oracle for
   non-empty payloads (prefer explicit empty-payload markers and/or
   length==0 exception that still rejects empty fields when length≠0).
   Verifier must PASS before re-review acceptance.

Do not treat (1)–(4) as external blockers: primary Java and the in-repo
verifier are available.

## Non-blocking / external (not used to reject)

- Authentic 289 game-cache pairing, live endpoint, credentials, live RSA/ISAAC
- Tutorial / guardian / random-event policy (out of milestone)
- Untracked tracer scripts under `tools/`
- Deeper inbound actor mask audit deferred to branchreviewer t_77d35bfb per orch
  comments (stage2 residual), not stage3 outbound/cache scope

## Created correction card

- **t_c2922712** — implementer: MESSAGE_PUBLIC effects + outbound field-oracle
  honesty + contract verifier empty-fields repair.
- `parents=[t_5f3a92c0]`, workspace `/Users/acfrazier/experiments/FR-client-289`.
- Required `kanban_request_review(reviewer="reviewer", ...)` after fix.
- Linked `t_c2922712` → parent of `t_77d35bfb` before this review completed.

## Summary

Stage3 rework closed the run496 draw-remap and unknown-length contract holes,
wired cache unpack through production `Client::load_cache`, and keeps 274 /
client-play green under the required native suite. Independent full-table
outbound field-oracle audit against primary Java still finds a **production
MESSAGE_PUBLIC effect encoding mismatch** plus dishonest/weak
SEND_SNAPSHOT/EVENT_MOUSE_MOVE contract rows. **REJECTED** pending one
serialized correction + re-review.
