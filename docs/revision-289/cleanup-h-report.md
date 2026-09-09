# Cleanup H: outbound input/lifecycle and ordered-byte coverage

Task t_34667a70, branch codex/revision-289-client. Base: accepted G
9344ee641e09e35a3e0408359e9faca15662be43 and test-isolation
c9faadeb673782880a27f7602a0d62a778f0230c. Implementation is ready for same-card
Grok4.5 review, not accepted or live-qualified. This report accompanies the
scoped H commit; the review handoff records its exact hash.

## Source, ownership and invariants

Primary public source is RuneWiki/openrs2-nonfree at
0c00ef249546fada67b1f6eb8bbe01ea7c250c95, revision289 client.java (J),
Applet_Sub1.java, Class11.java and Class1_Sub1_Sub3.java. J aliases resolve through
full-dispatch-audit.json. Isolated engine ClientGameProt.ts:28 corroborates empty
232; it is read-only evidence, not copied implementation. The accepted audit's
historical missing/mapped verdicts are retained; the table below is the H closure.

- The eight missing R289 operations now emit through actual owners:
  mouse-move/click, focus and camera immediately after inbound polling in
  game_loop and before click consumption; cycle7 after drag handling; idle after
  input/simulation and before keepalive; cycle4 at handle_chat_input entry;
  cycle5 only in the drawn mode2 crosshair path. No packet-end invention.
- Client::run owns a scoped 50ms recorder thread (Class11:19-23,45-52). It observes
  the GameShell pointer through a per-client mutex, caps the queue at500, drops
  only new samples when full, and joins on normal/early-return/unwind Drop.
  Direct step-driven clients can publish explicit observations with sample_mouse;
  no bot action API or host runtime was introduced. The real-driver test invokes
  Client::run and never calls sample_mouse; other deterministic tests inject
  public shell events/observations. None is live OS/window presentation proof.
- R289 pointer starts at Java int zero (Applet_Sub1:53-56). Mouse press latches
  separate click coordinates/time without overwriting the recorder pointer
  (349-359); move/drag/exit publish pointer changes, release/key/mouse events
  reset idle. Focus is forwarded from winit, blur clears held keys but not the
  key queue or idle count. R274 pointer construction and press/blur behavior stay
  unchanged. Mouse click is packed p4 with elapsed/50 capped4095, right-button
  bit19, and independently clamped X0..764 / Y0..502 linear coordinate.
- Camera keys1..4 latch pending input through a20-loop cooldown, then emit pitch
  before yaw (J:5909-5920), before that loop's camera simulation. Focus sends only
  transitions. Idle increments to>4500, sets logout250, subtracts500. Cycle4
  increments per keyboard poll call to>192 then resets, payload232; cycle5 counts
  mode2 crosshair draws to>57 then resets with empty85; cycle7 increments game
  loops to>62 then resets with empty232 (J:6023-6027).
- Cold login response2 clears click timestamp, duplicates, queue and idle and
  restores focused/last-reported-focus; coordinate/camera/cycle counters retain
  primary ownership (J:8411-8415,8430). The handshake fixture exercises login,
  then real game_loop input emission. Reconnect/legacy paths are not globally
  redefined.
- Draw-side tutorial/cycle1/3/6 ownership, CPU/GPU resource ownership and
  scene_state==1 last-FBO freeze remain intact. Existing action/walk/widget/drag,
  chat/social/design/report/map/keepalive payload writers are preserved.
  SEND_SNAPSHOT remains REPORT_ABUSE's R289 alias. R274 chat effects, report and
  default construction retain their existing revision branches and regression
  suites. Cycle2 retains its deterministic legal optional-choice/zero-value
  sequence, NOT Java randomness distribution equivalence.

## Explicit authorized primary framing correction

J:5829 tests payload_start - current_cursor <240, an ineffective reversed
subtraction. With500 recorded samples, literal Java can overflow method475's
one-byte payload length. Root explicitly authorized this R289-only correction in
the card: test forward emitted_payload <240 BEFORE the next WHOLE sample, retain
all unconsumed samples in order. Maximum payload is243. Do not describe this as
literal bug-for-bug equivalence. No stricter drop/flush/admission policy was added.

All three compression encodings retain source delta limits[-32,31], duplicate
interval split7/8 and saturation2047, raw(-1,-1) coordinate sentinel524287,
coordinate clamping and click-or40-sample admission. Goldens prove short encodings,
interval saturation/carry across packets,239+4=243 budget edge, exact240 stop,
500-sample splitting with ordered leftovers and unchanged501st-sample drop.
protocol-289.json and its promotion tool record the correction, proper camera
order, packed click fields, inclusive -32 delta and empty232. The new independent
outbound_lengths.rs source fixture includes empty232; no verifier was weakened.

## Ordered-byte and declaration evidence

All tests named below are in crates/client/tests/revision_289_outbound.rs. Each
operation has an actual action/UI/input/lifecycle emission witness, not a constant
hit. The40 action-family cases bind independent numeric opcode, declared length
and ordered payload (including every T/U field). Other tests assert full ordered
frames or source-ordered grammar for variable-random cycle1. The companion
all_82_declared_lengths_match_primary independently compares every declared
opcode/length, including zero232, with the source fixture; it supplements rather
than substitutes for these positive emissions.

Additional coverage: movement_signed_turns_through_ground_pick covers both signed
turn directions and run bit; movement_minimap_tail_and_isaac_concatenation checks
all14 minimap tail bytes and decodes adjacent ISAAC-encrypted operation/game walk
frames. Mouse/click boundary tests, cold login, real driver, disabled tracking,
focus transitions, camera pending/cooldown, idle repeat, all nine operation-counter
thresholds/no-reset and draw/loading counter gates are separate assertions.

The following82 unique operation names/IDs were checked programmatically against
the audit, actual production symbols and existing named test functions. Length -1
means a one-byte variable payload size; zero means opcode only. Source ordered
field requirements are those in full-dispatch-audit.json, with the explicit
mouse-budget exception above. OPLOC call arguments are additionally anchored at
J:11076/11011/11471/11412/11381-11384/11361/11289-11290.

| ID / operation / length | Primary ordered source | Current production emitter | Actual-path golden test |
| --- | --- | --- | --- |
| 4 OPOBJ2 / 6 | J:11442-11458 | crates/client/src/client/client.rs : 2759 | all_action_families_ordered_bytes_and_declared_lengths |
| 10 OPLOC1 / 6 | J:method160 call10; J:7445-7448 | crates/client/src/client/client.rs : 2953 | all_action_families_ordered_bytes_and_declared_lengths |
| 13 OPPLAYER3 / 2 | J:11519-11521 | crates/client/src/client/client.rs : 3042 | all_action_families_ordered_bytes_and_declared_lengths |
| 16 OPPLAYERU / 8 | J:11335-11339 | crates/client/src/client/client.rs : 3158 | all_action_families_ordered_bytes_and_declared_lengths |
| 21 OPNPC2 / 2 | J:11220-11232 | crates/client/src/client/client.rs : 2873 | all_action_families_ordered_bytes_and_declared_lengths |
| 22 OPOBJ5 / 6 | J:11429-11458 | crates/client/src/client/client.rs : 2778 | all_action_families_ordered_bytes_and_declared_lengths |
| 27 IDK_SAVEDESIGN / 13 | J:1953-1961 | crates/client/src/client/client.rs : 5600 | design_chat_modes_keepalive_and_map_completion |
| 30 OPNPC4 / 2 | J:11229-11232 | crates/client/src/client/client.rs : 2875 | all_action_families_ordered_bytes_and_declared_lengths |
| 34 CLIENT_CHEAT / -1 | J:10862-10864 | crates/client/src/client/client.rs : 9435 | social_keyboard_and_chat_ordered_frames |
| 40 OPHELD3 / 6 | J:11308-11316 | crates/client/src/client/client.rs : 3181 | all_action_families_ordered_bytes_and_declared_lengths |
| 44 INV_BUTTON1 / 6 | J:11542-11550 | crates/client/src/client/client.rs : 3307 | all_action_families_ordered_bytes_and_declared_lengths |
| 45 OPLOC2 / 6 | J:method160 call45; J:7445-7448 | crates/client/src/client/client.rs : 2962 | all_action_families_ordered_bytes_and_declared_lengths |
| 46 ANTICHEAT_OPLOGIC5 / 1 | J:11052-11053;11502-11503 | crates/client/src/client/client.rs : 3047; crates/client/src/client/client.rs : 3092 | operation_counter_boundaries_preserve_primary_no_reset |
| 49 ANTICHEAT_OPLOGIC4 / 1 | J:11060-11061;11513-11514 | crates/client/src/client/client.rs : 3033; crates/client/src/client/client.rs : 3100 | operation_counter_boundaries_preserve_primary_no_reset |
| 51 OPPLAYER2 / 2 | J:11508-11521 | crates/client/src/client/client.rs : 3039 | all_action_families_ordered_bytes_and_declared_lengths |
| 53 OPLOC4 / 6 | J:method160 call53; J:7445-7448 | crates/client/src/client/client.rs : 2975 | all_action_families_ordered_bytes_and_declared_lengths |
| 55 OPOBJU / 12 | J:11373-11379 | crates/client/src/client/client.rs : 2836 | all_action_families_ordered_bytes_and_declared_lengths |
| 67 MOVE_OPCLICK / -1 | J:10172-10196 | crates/client/src/client/client.rs : 3889 | movement_minimap_tail_and_isaac_concatenation |
| 69 OPPLAYER5 / 2 | J:11497-11521 | crates/client/src/client/client.rs : 3053 | all_action_families_ordered_bytes_and_declared_lengths |
| 73 ANTICHEAT_OPLOGIC6 / 2 | J:11539-11540 | crates/client/src/client/client.rs : 3304 | operation_counter_boundaries_preserve_primary_no_reset |
| 76 OPHELD1 / 6 | J:11305-11316 | crates/client/src/client/client.rs : 3175 | all_action_families_ordered_bytes_and_declared_lengths |
| 79 OPHELD5 / 6 | J:11294-11316 | crates/client/src/client/client.rs : 3192 | all_action_families_ordered_bytes_and_declared_lengths |
| 81 ANTICHEAT_OPLOGIC2 / 2 | J:11358-11359 | crates/client/src/client/client.rs : 2968 | operation_counter_boundaries_preserve_primary_no_reset |
| 85 ANTICHEAT_CYCLELOGIC5 / 0 | J:1778 | crates/client/src/render/draw.rs : 515 | cycle5_counts_only_drawn_mode2_crosshairs |
| 86 IF_BUTTON / 2 | J:11176-11177;11241-11242;11460-11461 | crates/client/src/client/client.rs : 3342; crates/client/src/client/client.rs : 3348; crates/client/src/client/client.rs : 3368 | widget_dialog_and_snapshot_ordered_frames |
| 88 ANTICHEAT_OPLOGIC9 / 3 | J:11299-11300 | crates/client/src/client/client.rs : 3186 | operation_counter_boundaries_preserve_primary_no_reset |
| 93 CLOSE_MODAL / 0 | J:2284 | crates/client/src/client/client.rs : 4864; crates/client/src/client/client.rs : 5900 | widget_dialog_and_snapshot_ordered_frames |
| 94 SEND_SNAPSHOT / 10 | J:1969-1972 | crates/client/src/client/client.rs : 5620 | widget_dialog_and_snapshot_ordered_frames |
| 97 OPOBJ1 / 6 | J:11439-11458 | crates/client/src/client/client.rs : 2756 | all_action_families_ordered_bytes_and_declared_lengths |
| 107 MESSAGE_PRIVATE / -1 | J:10789-10794 | crates/client/src/client/client.rs : 9354 | social_keyboard_and_chat_ordered_frames |
| 108 OPNPCT / 4 | J:11350-11352 | crates/client/src/client/client.rs : 2920 | all_action_families_ordered_bytes_and_declared_lengths |
| 110 OPOBJ3 / 6 | J:11445-11458 | crates/client/src/client/client.rs : 2762 | all_action_families_ordered_bytes_and_declared_lengths |
| 111 INV_BUTTON2 / 6 | J:11529-11550 | crates/client/src/client/client.rs : 3310 | all_action_families_ordered_bytes_and_declared_lengths |
| 112 OPHELDT / 8 | J:11136-11140 | crates/client/src/client/client.rs : 3272 | all_action_families_ordered_bytes_and_declared_lengths |
| 122 ANTICHEAT_OPLOGIC3 / 4 | J:11426-11427 | crates/client/src/client/client.rs : 2775 | operation_counter_boundaries_preserve_primary_no_reset |
| 124 INV_BUTTON3 / 6 | J:11526-11550 | crates/client/src/client/client.rs : 3313 | all_action_families_ordered_bytes_and_declared_lengths |
| 125 ANTICHEAT_CYCLELOGIC3 / 1 | J:4597-4598 | crates/client/src/render/draw.rs : 4220 | draw_counters_and_tutorial_ordered_payloads |
| 126 OPLOC5 / 6 | J:method160 call126; J:7445-7448 | crates/client/src/client/client.rs : 2979 | all_action_families_ordered_bytes_and_declared_lengths |
| 130 ANTICHEAT_CYCLELOGIC1 / -1 | J:7153-7176 | crates/client/src/render/draw.rs : 958 | draw_counters_and_tutorial_ordered_payloads |
| 133 ANTICHEAT_OPLOGIC7 / 4 | J:11436-11437 | crates/client/src/client/client.rs : 2753 | operation_counter_boundaries_preserve_primary_no_reset |
| 137 ANTICHEAT_CYCLELOGIC4 / 1 | J:10749-10750 | crates/client/src/client/client.rs : 9319 | cycle4_input_poll_counts_calls_even_without_keys |
| 138 OPPLAYERT / 4 | J:11029-11031 | crates/client/src/client/client.rs : 3134 | all_action_families_ordered_bytes_and_declared_lengths |
| 145 IDLE_TIMER / 0 | J:6071 | crates/client/src/client/client.rs : 11151 | idle_real_loop_threshold_and_repeat_subtract_500 |
| 146 TUT_CLICKSIDE / 1 | J:9852-9853 | crates/client/src/render/draw.rs : 3338 | draw_counters_and_tutorial_ordered_payloads |
| 147 OPOBJ4 / 6 | J:11453-11458 | crates/client/src/client/client.rs : 2770 | all_action_families_ordered_bytes_and_declared_lengths |
| 149 EVENT_APPLET_FOCUS / 1 | J:5922-5930 | crates/client/src/client/outbound_289.rs : 114 | focus_edges_clear_held_keys_without_idle_reset |
| 154 ANTICHEAT_CYCLELOGIC2 / -1 | J:7400-7420 | crates/client/src/client/client.rs : 3520 | deterministic_cycle2_is_a_legal_choice_sequence |
| 156 MESSAGE_PUBLIC / -1 | J:10923-10929 | crates/client/src/client/client.rs : 9522 | social_keyboard_and_chat_ordered_frames |
| 160 OPNPCU / 8 | J:11394-11398 | crates/client/src/client/client.rs : 2944 | all_action_families_ordered_bytes_and_declared_lengths |
| 161 CHAT_SETMODE / 3 | J:12269-12290;10801-10804;10946-10949 | crates/client/src/client/client.rs : 9251; crates/client/src/client/client.rs : 9264; crates/client/src/client/client.rs : 9277; crates/client/src/client/client.rs : 9371; crates/client/src/client/client.rs : 9570 | design_chat_modes_keepalive_and_map_completion |
| 166 RESUME_PAUSEBUTTON / 2 | J:11285-11286 | crates/client/src/client/client.rs : 3395 | widget_dialog_and_snapshot_ordered_frames |
| 168 ANTICHEAT_OPLOGIC8 / 1 | J:11450-11451 | crates/client/src/client/client.rs : 2767 | operation_counter_boundaries_preserve_primary_no_reset |
| 177 OPHELD2 / 6 | J:11311-11316 | crates/client/src/client/client.rs : 3178 | all_action_families_ordered_bytes_and_declared_lengths |
| 178 OPNPC3 / 2 | J:11226-11232 | crates/client/src/client/client.rs : 2874 | all_action_families_ordered_bytes_and_declared_lengths |
| 180 RESUME_P_COUNTDIALOG / 4 | J:10832-10833 | crates/client/src/client/client.rs : 9406 | widget_dialog_and_snapshot_ordered_frames |
| 181 NO_TIMEOUT / 0 | J:6130;10505;10523;10533;10536 | crates/client/src/client/client.rs : 10405; crates/client/src/client/client.rs : 10440; crates/client/src/client/client.rs : 10461; crates/client/src/client/client.rs : 10482; crates/client/src/client/client.rs : 11164 | design_chat_modes_keepalive_and_map_completion |
| 184 OPLOCU / 12 | J:method160 call184; J:7445-7448 | crates/client/src/client/client.rs : 3003 | all_action_families_ordered_bytes_and_declared_lengths |
| 189 OPPLAYER4 / 2 | J:11055-11056;11505-11521 | crates/client/src/client/client.rs : 3050; crates/client/src/client/client.rs : 3095 | all_action_families_ordered_bytes_and_declared_lengths |
| 191 OPHELD4 / 6 | J:11302-11316 | crates/client/src/client/client.rs : 3189 | all_action_families_ordered_bytes_and_declared_lengths |
| 192 IGNORELIST_ADD / 8 | J:5793-5794 | crates/client/src/client/client.rs : 5076 | social_keyboard_and_chat_ordered_frames |
| 193 EVENT_CAMERA_POSITION / 4 | J:5915-5920; pitch clamp J:5515-5520 | crates/client/src/client/outbound_289.rs : 107 | camera_arrow_latch_and_twenty_loop_gate |
| 195 ANTICHEAT_OPLOGIC1 / 4 | J:11008-11009 | crates/client/src/client/client.rs : 2959 | operation_counter_boundaries_preserve_primary_no_reset |
| 196 OPLOC3 / 6 | J:method160 call196; J:7445-7448 | crates/client/src/client/client.rs : 2971 | all_action_families_ordered_bytes_and_declared_lengths |
| 200 OPHELDU / 12 | J:11257-11263 | crates/client/src/client/client.rs : 3281 | all_action_families_ordered_bytes_and_declared_lengths |
| 203 FRIENDLIST_DEL / 8 | J:5035-5036 | crates/client/src/client/client.rs : 5097 | social_keyboard_and_chat_ordered_frames |
| 214 MAP_BUILD_COMPLETE / 0 | J:4507 | crates/client/src/client/client.rs : 10297 | design_chat_modes_keepalive_and_map_completion |
| 218 OPLOCT / 8 | J:method160 call218; J:7445-7448 | crates/client/src/client/client.rs : 2997 | all_action_families_ordered_bytes_and_declared_lengths |
| 220 OPPLAYER1 / 2 | J:11063-11064;11516-11521 | crates/client/src/client/client.rs : 3036; crates/client/src/client/client.rs : 3103 | all_action_families_ordered_bytes_and_declared_lengths |
| 224 EVENT_MOUSE_CLICK / 4 | J:5882-5907 | crates/client/src/client/outbound_289.rs : 94 | click_elapsed_units_and_button_clamps |
| 227 INV_BUTTON5 / 6 | J:11532-11550 | crates/client/src/client/client.rs : 3319 | all_action_families_ordered_bytes_and_declared_lengths |
| 229 EVENT_MOUSE_MOVE / -1 | J:5820-5878 | crates/client/src/client/outbound_289.rs : 32 | mouse_delta_interval_edges_and_tracking_disabled |
| 232 ANTICHEAT_CYCLELOGIC7 / 0 | J:6023-6027; E/network/game/client/ClientGameProt.ts:28 | crates/client/src/client/client.rs : 11096 | cycle7_real_loop_boundary_and_legacy_default |
| 234 MOVE_GAMECLICK / -1 | J:10164-10196 | crates/client/src/client/client.rs : 3881 | movement_minimap_tail_and_isaac_concatenation |
| 235 FRIENDLIST_ADD / 8 | J:8330-8331 | crates/client/src/client/client.rs : 5036 | social_keyboard_and_chat_ordered_frames |
| 236 MOVE_MINIMAPCLICK / -1 | J:10168-10196; J:12034-12044 | crates/client/src/client/client.rs : 3885 | movement_minimap_tail_and_isaac_concatenation |
| 241 OPOBJT / 8 | J:11482-11486 | crates/client/src/client/client.rs : 2813 | all_action_families_ordered_bytes_and_declared_lengths |
| 247 OPNPC5 / 2 | J:11223-11232 | crates/client/src/client/client.rs : 2876 | all_action_families_ordered_bytes_and_declared_lengths |
| 248 INV_BUTTON4 / 6 | J:11545-11550 | crates/client/src/client/client.rs : 3316 | all_action_families_ordered_bytes_and_declared_lengths |
| 251 IGNORELIST_DEL / 8 | J:12002-12003 | crates/client/src/client/client.rs : 5118 | social_keyboard_and_chat_ordered_frames |
| 252 OPNPC1 / 2 | J:11217-11232 | crates/client/src/client/client.rs : 2872 | all_action_families_ordered_bytes_and_declared_lengths |
| 253 INV_BUTTOND / 7 | J:6008-6012 | crates/client/src/client/client.rs : 5427 | inventory_drag_real_release_ordered_bytes |
| 255 ANTICHEAT_CYCLELOGIC6 / 1 | J:9096-9097 | crates/client/src/render/draw.rs : 727 | draw_counters_and_tutorial_ordered_payloads |


## Test-only transport readiness repair and preserved failures

The first H full all-features run FAILED (exit101,184.5s, root-confirmed receipt;
target/cleanup-h-workspace.log). Stage1 welcome_then_logout failed at its
feed_frames accepted-count assertion:0 vs1, NOT a generation-value mismatch.
The server-write barrier only guaranteed write_all completion, not client socket
visibility; the helper then did its prescribed single tcp_in poll and slept only
AFTER a false read. Other assertions were never reached. Preserve this failed run.

Root authorized a test-only readiness repair. feed_chunks now owns both local
socket ends synchronously, writes a chunk, waits at most2s for actual receiver
availability of total_written - bytes_in, performs exactly the prescribed parser
poll count, then allows the next chunk. This retains fragmentation barriers and
previous unread bytes without waiting for a whole logical packet. Server writes
also have a2s timeout. There are no unbounded barrier-wait threads to strand on
assertion/error paths; socket RAII handles cleanup. No production socket timeout,
framing/parser policy, packet/publication assertion or validator changed.

The explicit transport_readiness_counts_unread_bytes_before_fragment_poll
regression uses an unread prior byte plus a delayed next write and then fragmented
production CHAT_FILTER_SETTINGS with one parser poll per chunk. It passed1/1;
full stage1 subsequently passed47/47. This is a bounded transport-condition wait,
not repeated parser/test runs until green. The original socket race log is retained.

## Verification receipts

All Cargo commands ran serially in /Users/acfrazier/experiments/FR-client-289 with
CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target. The direct
subprocess wrapper records explicit argv/cwd/env/returncode and wall time in each
matching target/*.json; target/*.log contains unfiltered output. No shell pipeline
status inference, competing Cargo process, alternate build directory or live run.

- `cargo test -p client --test revision_289_stage1 transport_readiness_counts_unread_bytes_before_fragment_poll` → exit0; 1 passed, 2.92s. Receipt: `target/cleanup-h-readiness.json` and `.log`.
- `cargo test -p client --test revision_289_stage1 -- --test-threads=1` → exit0; 47 passed, 3.30s. Receipt: `target/cleanup-h-stage1.json` and `.log`.
- `cargo test -p client --test revision_289_stage2 -- --test-threads=1` → exit0; 48 passed, 2.99s. Receipt: `target/cleanup-h-stage2.json` and `.log`.
- `cargo test -p client --test revision_289_stage3 -- --test-threads=1` → exit0; 23 passed, 2.91s. Receipt: `target/cleanup-h-stage3.json` and `.log`.
- `cargo test -p client --test input -- --test-threads=1` → exit0; 16 passed, 0.64s. Receipt: `target/cleanup-h-input.json` and `.log`.
- `cargo test -p client --test revision_289_outbound -- --test-threads=1` → exit0; 24 passed, 2.42s. Receipt: `target/cleanup-h-edges.json` and `.log`.
- `cargo test -p client --test revision_289_outbound -- --test-threads=1` → exit0; 27 passed, 4.68s. Receipt: `target/cleanup-h-driver-turns.json` and `.log`.
- `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1` → exit0; 72 passing target summaries, 950 tests passed, 188.57s. Receipt: `target/cleanup-h-all-features-final.json` and `.log`.
- `cargo check --workspace --all-features` → exit0, 2.31s. Receipt: `target/cleanup-h-check.json` and `.log`.
- `python3 tools/verify_revision_289_contract.py` → exit0, 0.02s. Receipt: `target/cleanup-h-contract.json` and `.log`.

Full native workspace all-features includes GPU rendering suites, last-FBO tests,
legacy protocol/action/walk/logout/social/chat/input tests and client-play4. Earlier
H receipts from interrupted run918 (outbound22/input16/stage2 48/stage3 23 and
contract256/82/50) remain partial historical evidence; only the completed post-fix
workspace receipt is called passing. rustfmt on new Rust files and git diff --check
also pass. Existing large-file formatting is not broadly rewritten.

## Remaining gates and scope

Same-card reviewer=reviewer must review the exact H commit, especially the
explicit mouse budget exception, timer/input ownership and transport-test repair.
Root owns aggregate integration approval and required final whole-branch Grok4.6
review. Authentic289 cache/server pairing, approved endpoint/account, live
RSA/ISAAC compatibility and actual login/action/logout/presentation remain unproven.
Offline tiny public fixtures and injected events do not qualify those live gates.
No server/script/cache/account/host-bot work, shared fixtures, remote/Windows/live
execution, merge, push, remotes, submodules or new cards were used. The six
inherited unrelated untracked review/tracer artifacts remain unstaged.

hotspot: crates/client/src/client/client.rs — shared campaign ownership remains
serialized; H changes only its named input/lifecycle/counter integration sites.
