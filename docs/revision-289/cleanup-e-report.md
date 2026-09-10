# Cleanup E: atomic world and zone dispatch

Task: t_3aa4481e. Foundation: 5f5148b (Cleanup D, accepted on
 t_115f2843). Branch: codex/revision-289-client. Implementation complete;
same-card Grok4.5 review is pending. This is offline packet/lifecycle proof,
not authentic cache/server or live presentation compatibility.

## Sources and scope

Public primary: RuneWiki/openrs2-nonfree, revision 289, pin
0c00ef249546fada67b1f6eb8bbe01ea7c250c95. Read client.java method149
(lines 6794-7050), full-follows (2755-2773), and rebuild (2999-3125),
against the E rows in full-dispatch-audit.json. Fixtures are independently
constructed byte arrays, not captured private traffic. Merge model fixture
is a minimal zero-geometry 18-byte model with explicit loc dimensions.

Owned IDs: 60, 71, 83, 87, 90, 91, 106, 112, 117, 144, 155, 176,
194, 219, 233. No F-H changes, host/server changes or renderer ownership
changes. Global synth177 remains G-owned. R274 decoder/public constants
remain intact.

## Implementation

- New private zone_289 module stages all eleven zone operations. The whole
  enclosed frame, including unknown IDs, truncation, shape and referenced
  config indices, validates before origin changes or any inner apply.
  Apply consumes typed values, never Packet. No Client/World cloning,
  double parsing or rollback. The old R289-to-R274 zone decoder alias and
  unknown-tail skip were removed.
- Both direct and enclosed paths handle object add/reveal/delete/count,
  loc add/delete/animation/merge, projectile, spot animation and area sound.
  Publication flags coalesce once per outer frame; successful merge adds
  player to scene. Area sound itself does not stamp scene/player.
- Area sound always consumes all four bytes; inclusive route-tile radius,
  loops low three bits, wave-enabled/non-lowmem/queue-under-50 gates and
  delay lookup match primary behavior.
- Full-follows clears the bounded ground region and expires matching
  pending locs on the active plane, leaving other planes/regions alone.
- LOC_ANIM uses primary 0..103 exclusive tile bounds, correct southeast
  corner and existing deferred model/stamp ownership. The R274 path keeps
  its established semantics.
- Merge retains local self-slot versus remote storage, model attachment,
  rotated footprint, normalized signed bounds and relative scheduled times.
  Projectile deltas/target remain signed; receiver/count/masked-ID object
  semantics are tested. R289 pile valuation uses Java wrapping arithmetic
  so valid large stacks cannot panic after staged application begins;
  R274 arithmetic is unchanged.
- Rebuild decodes coordinates as a typed operation, then invokes extracted
  packet-free shared application. Async requests, translations, cache
  ownership and scene_state=1 freeze lifecycle remain in the existing body.

## Production-path fixtures

crates/client/tests/revision_289_zones.rs: eight tests, each exercising the
production dispatcher. Multi-case tests cover all eleven operations in both
standalone and enclosed forms:

- area_sound_direct_and_enclosed_consumes_and_obeys_primary_gates
- objects_direct_and_enclosed_receiver_count_mask_and_pile
- loc_changes_direct_and_enclosed_and_full_zone_expiry
- loc_anim_direct_and_enclosed_uses_primary_edge_and_southeast
- projectile_and_map_anim_direct_and_enclosed_signed_fields
- merge_direct_and_enclosed_attaches_local_and_remote_and_schedules
- enclosed_rejects_unknown_short_and_invalid_tail_before_sound_prefix
- rebuild_translates_local_and_pending_state_and_keeps_world_until_load

Coverage includes follows origins, plane/edge distinctions, signed target
and deltas, local/remote attachment and schedules, mixed merge+91 frames,
scene+player publication once, unknown/truncated/invalid-config suffixes
without sound-prefix mutation or generation publication, and rebuild
translation/world/cache retention. Invalid admission still permits the
existing T2/lifecycle failure path; it must not publish semantic changes.

## Exact verification

All Cargo runs use
CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target.

Final passing commands:

- cargo test -p client --test revision_289_zones -- --test-threads=1
  (8 passed).
- cargo test --workspace --no-fail-fast -- --test-threads=1
  (exit 0; target/cleanup-e-workspace-final.log).
- cargo test --workspace --all-features --no-fail-fast -- --test-threads=1
  (exit 0; target/cleanup-e-all-features-final.log).
  Includes lib76, actors13, stage1 46, stage2 42, stage3 23, zones8,
  server_packets19, legacy zone45 and client-play4. All-features exercises
  the feature-gated regression suite, not a new live GPU presentation run.
- cargo check --workspace --all-features (exit 0).
- python3 tools/verify_revision_289_contract.py
  (PASS: 256 inbound, 82 outbound rows; 50 fixtures).
- rustfmt --edition 2021 on zone_289.rs and revision_289_zones.rs;
  git diff --check (PASS). Existing broad client.rs formatting debt was
  not reformatted as a drive-by change.

Failed/partial runs retained rather than relabeled green:

- TDD failures before each missing operation/apply extension; invalid-config
  suffix exposed a sound-prefix leak, fixed by pre-apply validation. Large
  stack fixture exposed debug overflow, fixed by revision-gated wrapping.
- Fixture setup compiler fixes: Arc-backed delay mutation, new_angle field;
  merge model fixture needed explicit replacement of prepopulated loc data.
  One borrow-check failure while gating pile arithmetic was fixed before
  final full-suite runs.
- target/cleanup-e-workspace.log: initial native run hit the 180s command
  cap and included a stage1 failure; not a completed workspace gate.
- target/cleanup-e-all-features.log: earlier completed run failed stage1
  fixed_empty_sync_uses_isaac_inbound_and_publishes_varp and
  welcome_then_startup_script_reaches_first_tick_with_correct_framing.
  target/cleanup-e-focused.log also records stage1 failure. Fixed-empty-sync
  load sensitivity is pre-existing per D review; welcome failure was also
  transient here. No test timing code was changed. Final native and
  all-feature workspace runs passed both tests.

## Remaining gates

Same-card reviewer=reviewer approval comes next. Root owns subsequent F-H,
authentic cache/server pairing, live RSA/ISAAC and presentation proof, and
required final whole-branch Grok4.6 review. Compilation and fixture success
are not substitutes. No remote/live session, merge or push was performed.

Hotspot: crates/client/src/client/client.rs remains serialized shared work;
E touches only dispatch wiring, rebuild extraction and R289 pile valuation.
