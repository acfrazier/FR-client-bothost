# Revision 289 complete dispatch cleanup contract

Corrective audit for t_43f9ff17, against client a97b4ae. Docs only; no code or live run. Supersedes the rejected a97b4ae template ledger, not its review receipt. Implementation requires actual same-card Grok4.5 approval and root release; this document does not authorize another live attempt.

## Evidence and inventory

`full-dispatch-audit.json` schema 2 is the operation ledger. J/R/D/E/C aliases resolve to exact files in its `sources`. Each inbound row contains ordered fields, primary Java branch, actual R289 handler or absence, 274 semantic ID/handler, concrete differences, state/redraw, current/required generation, verdict, and emission edges. `emission_witnesses` groups concrete engine/content reachability. This is a protocol-operation audit, not a claim to have executed every script or traversed every possible script state.

Primary: RuneWiki/openrs2-nonfree 289, pin `0c00ef249546fada67b1f6eb8bbe01ea7c250c95`, local `/Users/acfrazier/experiments/FR-vault/research/deob/289/nonfree/client/src/main/java/`. Read/write primitives are in Class1_Sub1_Sub3; no transformed endian/add/sub reads appear in these inbound fields. Java dispatch J:2593-3546 has 70 distinct IDs, including eleven shared direct-zone branches. There are 186 unsupported byte IDs, not 186 missing handlers. In particular ID122 has table length6 but no Java branch; a nonzero length does not prove an operation either. Unsupported IDs reach J:3547-3548 T1/logout. Class17:11 supplies lengths, not semantic validity.

Current 70-operation verdicts: 2 complete, 41 partial, 27 missing. Complete means the audited source semantics/publication are accounted for (LOGOUT and VARP_SYNC), not live acceptance. Partial includes mapped zone operations that work only enclosed, generation omissions, and strict-frame weaknesses. All 82 named outbound operations have fields and emit evidence or explicit absence: 73 mapped, 1 partial (deterministic cyclelogic2 choices), 8 missing actual emitters. Mapped is not a runtime result.

## Design decisions before implementation

1. Preserve R274 as construction default, its public constants, packet tables, valid-payload behavior and established tests. Keep R289 explicit for the life of a session. Never feed a raw 289 ID to a 274 match. Shared semantic apply routines are permitted only after a revision decoder has produced the same ordered meaning.
2. Use an internal semantic operation representation for R289, not a public bot action API. Decode an exact bounded frame into validated data; apply once; return the affected generation set. Keep the existing 274 path intact initially. Do not add a second uncoordinated numeric generation match that can drift from dispatch again.
3. Fixed payloads must match exactly, including zero-length packets. Variable strings terminate inside their declared frame; ignore lists are divisible by8; private chat has at least13 bytes; actors respect bit and byte bounds; inventory minimums are explicit. Production psize0 must never mean the backing allocation length. Existing inventory staging/bounds for nonzero psize are useful and must survive.
4. Validate the whole frame before publishing semantic mutations. An enclosed zone frame validates all inner IDs/lengths before applying any inner effect. Unknown top-level IDs fail closed; unknown inner IDs fail the whole frame rather than silently skipping the remainder. Report bounded diagnostics and preserve T1/T2 reset behavior; never log raw private-message or login-IP payloads.
5. Explicitly consumed padding is not an invented field. HINT_ARROW type1/type10 consumes three padding bytes; stop/other types consumes five. Do not require zero padding when Java does not. Fixed six-byte framing must remain true for every variant.
6. Observation generations are client publication, not Java protocol fields. On success, bump each affected family at most once for an outer packet (including enclosed frames). On T1/T2/logout, invalidate all through the existing reset. No valid-prefix publication or extra success-family bump after failure. Ordinary retained no-op operations may still invalidate their declared family, matching the existing conservative model.
7. Required families are row `gens[1]`: inventory, varp, stat, iface, chat, scene, camera, map_flag, world, player, npc; rebuild/logout all. Reset-anims requires player+npc; loc-merge scene+player; player/private/friend messages add chat only when they publish a log change. Hints/minimap/reboot/audio remain directly observed state with no new public generation family. Redraw and generation are separate obligations.
8. Keep per-client interface overlays and Arc ownership. Never mutate shared cache definitions. World/collision/model stamps update at the existing simulation application point; render owns CPU/GPU resources. `scene_state==1` must retain last-FBO freeze. Do not make packet handlers upload meshes, allocate renderer textures, or send host bot actions. Existing draw-side tutorial/cyclelogic emits stay where they are; this task is not a renderer refactor.

## Bounded implementation sections

Sections below partition all 70 inbound operations; an ID belongs to one section even when its side effects span families. Serialize changes to `crates/client/src/client/client.rs` (hotspot), and require section tests plus independent review. Root chooses dispatch cards after approving this contract.

### A. Frame, publication and lifecycle foundation

IDs: 120, 121, 253.

Implement bounded decoder/apply/publication discipline first and retain UPDATE_PID self-slot/membership and LOGOUT. LAST_LOGIN_INFO is presently consume-only in R289 and a 274 no-op, not parity-complete. Implement its ten-byte fields and Java conditional welcome-interface selection (clientcode650/655), modal closure and display state without moving DNS/network work into rendering or printing the IP. Keep optional DNS display resolution under lifecycle ownership. Do not silently exclude map_live: isolated login script explicitly emits it conditionally.

Tests: fixed empty packet; every fixed nonempty packet short/long through bounded dispatch; variable zero size; reusable-buffer stale bytes; partial socket reads and adjacent frames; malformed frame after a valid frame; unchanged semantic state before failure reset; all-family reset exactly through lifecycle; same 274 sessions still default correctly.

### B. Interfaces and tutorial state

IDs: 12, 18, 23, 30, 35, 55, 59, 63, 79, 81, 119, 127, 138, 160, 181, 184, 189, 211, 222, 244, 252.

Route the missing modal/model/scroll/hide/tutorial/count operations to bounded semantic equivalents. Correct tab-clear 65535 to -1 (current R289 inline copy lost the legacy helper conversion). IF_SETOBJECT65535 clears model type; preserve signed offsets/sequence/overlay IDs. IF_SETPLAYERHEAD requires Java transformed-NPC appearance handling, not only ordinary kit-derived head IDs. Keep distinct animation reset behavior for single-modal opens versus main+side. Implement tutorial message storage/render consumption together with section F's kind0 message path. Preserve per-client overlay isolation and publish iface generations even for fields Java drew without explicit dirty flags.

Tests: every row exact byte fixture; modal transitions with prior side/chat/count states; signed -1 clears; zero and nonzero object scale contract; active/inactive tab dirty behavior; head model choices; scroll clamp order; tutorial flash/ack path; two-client overlay isolation; generation snapshots; no stale CPU/GPU modal chrome.

### C. Inventory, variables and stats

IDs: 28, 46, 75, 76, 97, 107, 154, 172, 195.

Retain full count u16 versus legacy u8 and partial slot uSmart versus legacy u8. Preserve nonzero-frame per-field bounds/staged publication; eliminate psize0 allocation fallback. Add stop-transmit final item-ID clearing (counts unchanged in Java). Add missing stat and inventory publication. Varp small is signed i8; large is i32; sync reconciles rather than zeros. Restore tutorial chat redraw on small/large changed varps without altering 274 behavior.

Tests: sparse slots across127/128 and255/256, extended counts, truncated later entry atomicity, zero-declared frame with stale backing data, empty full inventory, out-of-range slots consumed, signed weights/vars, derived base levels, unchanged-var no extra redraw, family changes and 274 width regressions.

### D. Actors

IDs: 65, 188, 201.

Use row bit/mask order, not mask-value heuristics. Shared Rust already binds NPC CHANGETYPE metadata; retain it. Gate hit health-bar timeout to Java cycle+300 for R289, preserving 274+400. Restore player SAY leading-tilde/local-player chat behavior and corresponding chat publication. Preserve appearance/animation/spot/exactmove semantics, local actor ownership, and reset-anims invalidation of both actor families. Decode before mutation; a final cursor check is not enough for malformed masks.

Tests: local/old/new movement, retained/removal counts, sentinel and no-new-record bit endings, every single mask and combined masks, transformed appearances, CHANGETYPE metadata, hit timers, SAY local/remote/tilde, public chat flags, animation priority/reset, exactmove timing, truncated combined mask, actor reset and 274 regressions. Existing stage fixtures are evidence for their covered bytes, not a waiver for these new findings.

### E. World and zone protocol

IDs: 60, 71, 83, 87, 90, 91, 106, 112, 117, 144, 155, 176, 194, 219, 233.

Implement both direct routing and enclosed routing for all eleven primary zone operations. Existing ten-entry translation silently skips unknown tails; replace it with strict semantic decoding. Area synth91 is the additional primary operation absent from legacy Rust and the isolated engine's ten-entry zone table. Respect radius/loops/queue/audio gates while always consuming four bytes. Restore pending loc expiration on full-follows144 (existing R289 only clears ground objects). R289 LOC_ANIM uses Java tile bounds and actual southeast height rather than the documented legacy northeast copy. Retain loc-merge actor attachment and scheduled start/end behavior. Rebuild preserves async map lifecycle, translations, client caches and last-FBO freeze.

Tests: all eleven standalone and enclosed operations; mixed inner frames including91; unknown inner after valid prefix must not publish; short inner fields; direct follows origin; full zone pending-expiry; plane/edge bounds; signed projectile target/deltas; merge start/end and local slot; object receiver/count behavior; rebuild freeze and cache-lifecycle regressions. A table containing every ID is not a substitute for these dispatch paths.

### F. Social, chat and player options

IDs: 13, 21, 47, 168, 196, 235, 243.

Implement friend updates/private messages and bounded strings/lists. Preserve sorting, message deduplication, ignore/staff gates and WordPack. MESSAGE_GAME requires chalreq as well as trade/duel, and kind0 tutorial message plus click clearing; the current helper explicitly lacks those Java behaviors. Publish chat for actual friend notices, inbound player chat and private/game messages. Keep option null/index/priority handling and filter redraw.

Tests: ignore-list remainder/capacity, private header minimum and dedup/ignored exact consumption, staff levels, each request suffix, tutorial kind0, current-world sorting, player-option null/index edges, filter modes and successful generation visibility.

### G. Camera, audio, hints and miscellaneous HUD

IDs: 29, 73, 82, 115, 133, 136, 164, 177, 187, 204, 208, 247.

Reuse field-proven camera/sound semantics with explicit bounds and required publication. Camera look/move snap threshold100 and pitch clamp128..383; shake axis0..4. Implement HINT_ARROW six-byte type1/2..6/10/other, tile offsets and normalization to2. Engine stop255 is an unsigned disabling type, not a signed field. Add map-flag clear, multiway, reboot timer and audio selection/queue gates; do not confuse global synth177 with area synth91 owned by E.

Tests: each hint variant including nonzero ignored padding, exact-six rejection, NPC/player hints and clear; camera snap/ease/shake/reset; music sentinel/lowmem/disabled/delay; synth queue capacity; reboot scale; minimap/HUD behavior. No claim that valid packet state alone proves audible output or real GPU presentation.

### H. Outbound and final offline coverage

All 82 JSON outbound rows. Existing actual emit sites cover actions, walking (including minimap14-byte tail), widgets, inventory drag, social/chat, design, report, map acknowledgement, tutorial acknowledgement, keepalive and several counter packets. Preserve SEND_SNAPSHOT's REPORT_ABUSE alias and R289-only chat effect behavior. Add the eight missing input/lifecycle emits: IDLE_TIMER, mouse click/move, focus, camera position, cyclelogic4/5/7. Camera is pitch then yaw; click is one packed p4 (time/button/linear coordinate), not p2 X/p2 Y. Cyclelogic7/232 is EMPTY in primary J:6023-6027 and isolated engine ClientGameProt.ts:28, not length1 as declared by the current Rust table/old manifest. Record deterministic cyclelogic2 as a permitted choice sequence, not Java randomness equivalence. Correct old `protocol-289.json` field labels/length232 and source fixtures in implementation, not by weakening verifiers.

Tests: golden emitted opcode+length+ordered bytes through actual UI/action paths; no constant-only positives; ISAAC/framing concatenation; all action T/U extra fields; both movement coordinate signs/tail; telemetry packing branch boundaries; input focus/idle/camera gating; preserve 274 report/chat behavior. Then serialized stage1/stage2/stage3 suites, full native workspace including client-play, required GPU gates and contract verifier. Root obtains actual Grok4.5 section/aggregate approval and required Grok4.6 whole-branch review before reopening live proof.

## Reachability and remaining prerequisites

The engine/content tree is read-only. The JSON names login conditionals, resumed tutorial, queues/timers, every interface command edge, actor tick publication, direct/enclosed zone emission, social and audio/camera witnesses. These are conservative source reachability, not an assertion that every branch fires for one account. No authentic fixture exclusions were established. Area synth91 is primary-valid without an isolated engine emitter; the other operations stay required even if no current test login exercises them.

Genuine additional capabilities relative to the existing Rust implementation are area synth, transformed player-head support, tutorial-message/challenge/SAY side effects, welcome-info publication and the absent outbound telemetry/lifecycle paths. Not all are historically revision-only packets: welcome-info and telemetry already have 274 names but no behavior. Remapped names such as IF_SETTAB/IF_SETICON are not new capabilities.

Still separate: authentic server/cache pairing, authorized endpoint/account and live presentation proof. No host integration, bot API, server/script policy changes, remote changes or copied private source belongs in this cleanup. Useful offline implementation does not require live credentials; live acceptance must not be inferred from compilation, the 256-entry length table or current fixtures.
