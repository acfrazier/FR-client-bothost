# Reviewed native ground movement observation

2026-09-09. Bounded functional observation, not complete revision289 acceptance.

The reviewed trace implementation is a7f49098e9bf54567807ac58d337869fd10abba0.
Same-card t_2ebebe02 was approved by actual Grok4.5/xai075229_e64b14; required
final t_99cec734 by actual Grok4.6/xai075729_f8ae77. Final review document commit
08a44be7e6ad36327353c978f3103db266809a4f was the source HEAD for the fresh build.
Build: cargo build --locked -p client-play --no-default-features --features window.
Executable SHA256: a8bab7943c898cf151fe1f25437b843b060747d6c7ea0efda6f73134ec6347c8.
Build receipt: target/ground-trace-reviewed-live-build/receipt.json.

Preflight rechecked protected config/cache hashes and isolated engine/content
pins; ground-trace-native-preflight-20260909.json retains that result. The owned
launcher admitted exact HEAD, executable hash and clean tracked source, copied
the executable into a new native app bundle, and bounded lifetime to300s.
CLIENT_289_GROUND_TRACE=1 was the sole diagnostic opt-in. Window-only build:
audio is untested. Startup logged wgpu and ingame; snapshot-load-skipped warning
remains in the log and is not silently removed.

## Observation

Root directly read CUA screenshots. One left click on visible clear floor at
app coordinates88,143 produced native inner physical176,230, inner766x504,
scale2. The trace has exactly15 lines, one id, and confirms scene2 at dispatch.
It selects WALK, arms172,226 after viewport inset, observes post-render tile53,49,
and routes from54,50 at base3040,3056 onplane0. Movement type0, opcode234,
length5, run0, absolute3093,3105, turns1; route succeeds without nearest fallback.
The write API returns Ok, ending the trace. Independently, tile logs change
from3094,3106 atcycle1900 to3093,3105 atcycle1950 and retain the new position
throughcycle3950. Before/after screenshots show the changed player/scene position.

Root opened the logout tab, clicked the logout button once, and directly saw
the login title screen. Native window close exited0 after92.39726209640503s.
External process inspection confirmed ownedPID25841 absent; no post-close CUA
call was made. No T1/T2 log lines were seen. Server49444 remains root-owned.

## Preserved evidence and limits

Raw directory: /Users/acfrazier/experiments/lostcity-289/runtime/client-proof-groundtrace-1212.
Its proof-manifest.json hashes client.log, launch.json and completion.json.
client.log SHA256: 6e6a8e59dd986ceec91ceeb28fb32e4d14e470f7a435856a4e36e2d0bb403032.
CUA screenshots were directly inspected in the conversation; no local images
were archived. This observation establishes this single native displacement
and logout, not a wire/server receipt from API Ok alone. Earlier failed click
receipts remain preserved and their exact cause remains unknown. The diagnostic
did not fix movement behavior. Pre-scene camera values are not asserted to be
the actual terrain-pick camera. NPC Talk-to, full tutorial, audio, full client
parity, numerical latency/cadence and memory/performance remain unproven. No
integration with274bot is accepted or released by this observation.
