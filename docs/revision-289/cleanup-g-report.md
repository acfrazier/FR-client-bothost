# Cleanup G: camera, audio, hints and HUD

Task t_7f7981c3, branch codex/revision-289-client. Accepted F foundation:
97341e64aea648b1d92d7f2a10e60080ff682f17 (actual Grok4.5 review on t_4583da40).
This is Section G only; no H, live, host/server/cache, remote, merge or push work.

## Implementation and source evidence

Primary public source: RuneWiki/openrs2-nonfree
0c00ef249546fada67b1f6eb8bbe01ea7c250c95, revision289 client.java (J), with
Class17:11 fixed lengths. The twelve rows were checked against
full-dispatch-audit.json. Fixtures are independently chosen minimal bytes in
crates/client/tests/revision_289_misc.rs, not copied server traffic or private
cache material.

The internal misc_289 module extends R289Operation: fixed-frame admission,
validated semantic decode, exact-end check, packet-free application and returned
publication. No raw R289 ID reaches the R274 dispatcher. R274 remains default;
its public constants, helpers and valid behavior are unchanged.

| ID | Primary anchor | Behavior |
| --- | --- | --- |
| 29 | J:3316-3327 | Jingle u16 ID/u16 delay; no sentinel conversion; enabled/!lowmem gates, fading false, existing on-demand request owner |
| 73 | J:2804-2818 | Camera move fields; terrain height, rate2>=100 snap, otherwise existing cinema_camera easing |
| 82 | J:3417-3443 | Camera look fields; snap yaw/pitch, clamp128..383, otherwise existing per-loop easing |
| 115 | J:2684-2720 | Six bytes for every unsigned type; actor slot plus three ignored padding bytes; tile types2..6 offsets and normalization to2; other types consume five ignored bytes |
| 133 | J:3518-3525 | Cinema off, all five shake enables cleared |
| 136 | J:2734-2738 | Unsigned minimap state; existing drawing and click gates |
| 164 | J:2912-2916 | Clear minimap flag x only |
| 177 | J:3328-3340 | Global synth ID/loops/delay, enabled/!lowmem/queue<50; validated queue and config indices, checked base-delay addition |
| 187 | J:3302-3315 | Song65535 -> -1; next song updated even when enabled/lowmem/delay/same-song gate suppresses request |
| 204 | J:3154-3158 | Reboot u16 multiplied30 |
| 208 | J:2959-2971 | Shake axis0..4, parallel-array validation, parameters and cycle reset |
| 247 | J:3466-3470 | Unsigned multiway flag; R289-only headicons1 HUD paint at472,296 from J:1799-1800 |

Camera operations publish camera once; map-flag clear publishes map_flag once;
multiway publishes world once. No invented hint/minimap/audio/reboot family.
Malformed frames and semantic indices fail before packet effects, then use the
existing all-family lifecycle reset. Audio queue/config validation is performed
when the Java gate actually admits an enqueue. On-demand requests keep their
existing valid-file/dedup checks. Renderer resource ownership, CPU/GPU split and
scene_state==1 last-FBO freeze are unchanged.

## Production-path fixture coverage

All ten tests dispatch through Client::handle_packet. The helper checks exact
consumption and full eleven-family generation snapshots for every successful
R289 packet, not just the expected changing family.

- All twelve fixed frames reject every shorter size and one longer size, with
  valid bytes retained in backing storage; semantic state and reset generations
  are checked. Zero-byte reset/flag frames reject a nonempty frame.
- All256 hint type bytes, nonzero ignored padding, NPC/player slots, five tile
  offsets, normalization, active-hint clear via0/255; exact-six rejection for
  actor/tile/clear variants; invalid NPC/player indices remain atomic.
- Camera move/look threshold99/100, real cinema_camera easing (look250 ->228),
  bridge-plane bilinear terrain height, pitch lower/upper clamp, all five shake
  axes, invalid5/255, reset and invalid plane before mutation.
- Song sentinel and enabled/lowmem/delay/same-song gates; jingle unsigned65535,
  delay and fading; actual on-demand outstanding request counts and delayed
  return to the next song through sounds_do_queue; truncated jingle issues no
  request before lifecycle reset.
- Synth queue49/50, disabled/lowmem gates, cache base delay, invalid sound ID,
  negative queue count, shortened parallel queue, absent config and overflow.
- Flag x-only clear, reboot scale, unsigned minimap/multiway values; actual
  minimap click suppression/enabled emit, minimap state2 pixel blanking, and
  multiway on/off pixel assertion using an independent one-pixel sprite.
- Default R274 camera, hint's legacy prefix-only cursor, multiway, song sentinel
  and reboot dispatch use unchanged legacy constants.

## Verification and preserved interruptions

All builds/tests use CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target.
No simultaneous Cargo runs were started in this resumed attempt.

- Earlier `cargo test --workspace --no-fail-fast -- --test-threads=1`: exit0,
  199.3s per preserved root receipt; target/cleanup-g-workspace.log ends with
  legacy zone45, client-play4 and successful doc tests.
- Earlier all-features run: interrupted exit130 after158.4s per root receipt;
  target/cleanup-g-all-features.log stops mid-stage1. Not a pass.
- The prior resumed all-features output target/cleanup-g-all-features-resume.log
  finishes with THREE failed targets, not a pass: stage1 welcome/logout expected
  accepted packet1 but got0 at revision_289_stage1.rs:1117; stage2 enclosed zone
  fails invalid loc and leaves origin0,0 instead of40,48; E zones fails four
  tests at config bounds for loc/object/spotanim. Its zombie process is exited,
  not an active build lock.
- Fresh `cargo test --workspace --all-features --no-fail-fast -- --test-threads=1`:
  FAILED, completed log target/cleanup-g-all-features-final.log. Two failed
  targets remain: stage2 is47/48 (enclosed loc fixture), E zones4/8 (the same
  four config-bound fixtures listed above). Stage1 now46/46, G10/10, lib76,
  stage3 23, GPU suites and client-play4 pass. The process tool returned a null
  exit code incorrectly while Cargo was still running; final failure is grounded
  in the completed Cargo summary, not an invented numeric exit status.
- `cargo test -p client --test revision_289_misc --test iface_model --test gpu_texture --lib -- --test-threads=1`:
  exit0, G10/10, iface_model9/9, gpu_texture10/10 and lib73/73 (including
  last-FBO freeze). Log target/cleanup-g-focused-final.log. This includes the
  preserved hint clear/exact-six and look-ease228 additions after restart.
- `cargo check --workspace --all-features`: exit0; client and client-play.
  Log target/cleanup-g-check-final.log.
- `python3 tools/verify_revision_289_contract.py`: PASS256 inbound/82 outbound/
  50 fixtures. This checks the existing contract inventory, not G behavioral
  coverage; the production fixtures above supply that evidence.
- `rustfmt --edition 2021 --check crates/client/src/client/misc_289.rs crates/client/tests/revision_289_misc.rs`: PASS.
- `git diff --check`: PASS.

The E fixture construction uses cache_dir=/tmp and does not populate the config
IDs used by those positives. Client::load_cache falls back to Cache::default
(empty tables), while zone_289 correctly requires indices below table lengths.
These fixture, cache and E validation files, and the constructor body, are
unchanged from accepted F97341e6. This is source attribution, not a separately
executed baseline build. No cache artifacts were created or validation weakened to
make those tests pass. The welcome/logout failure is in bounded socket polling;
its precise scheduling cause is not established by the assertion alone.

## Remaining gates

Same-card reviewer=reviewer must review the committed G change. Root retains
H authorization, authentic cache/server pairing, live RSA/ISAAC and presentation
proof and the required final whole-branch Grok4.6 review. Packet state and
synthetic pixel/request-count fixtures do not establish audible output or real
GPU/server presentation. Inherited untracked review/helper files remain outside
the commit. Hotspot: crates/client/src/client/client.rs remains serialized.
