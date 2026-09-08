# Revision 289 full dispatch cleanup contract

Status: source/handler audit only; no code or live proof is authorized by this card.

## Evidence boundary

The machine-readable ledger in `full-dispatch-audit.json` covers all 256 wire IDs from the pinned primary 289 Java length table (`Class17.anIntArray209`) and all 82 outbound rows already recorded in `protocol-289.json`. Every inbound row keeps its primary source anchors and independently records the Rust dispatch/handler anchor. A size constant is never treated as a handler proof. Zero-length rows remain valid protocol rows.

Source pin: RuneWiki/openrs2-nonfree branch `289`, commit `0c00ef249546fada67b1f6eb8bbe01ea7c250c95`; entry `client.java`. The 289 Java packet reader is `client.java:2553-2587` and dispatch begins at `client.java:2593`; the canonical 256-length table is `Class17.java:11`. Rust revision declarations are `crates/client/src/io/revision.rs:46-103`; R289 dispatch is `crates/client/src/client/client.rs:6698-6998` and the shared zone translation is `:7841-7863`.

Current audit totals: 22 complete, 21 partial, 213 missing. “Partial” means framing and/or a named revision constant exists but field/state proof or a Rust handler is incomplete. “Missing” means the R289 dispatcher deliberately consumes/fails closed without a semantic handler. These are audit verdicts, not claims that all 256 IDs are expected during ordinary startup.

## Cleanup contract

1. Keep `ClientRevision::R274` as the default and preserve the public 274 opcode/length tables. All shared paths that differ by message effects, payload ordering, or redraw semantics must branch on revision rather than inherit R289 behavior.
2. Keep strict framing: select the revision table at session construction; fixed lengths must consume exactly that many bytes; variable and extended frames must verify their declared boundary before dispatch. Unknown R289 IDs remain fail-closed and consume their bounded frame, preventing a wrong 274 handler or stream desynchronization.
3. Implement in semantic sections, not numeric order: (a) lifecycle/login/reset and generation ownership, (b) actor/player/NPC updates, (c) scene/rebuild and zone inner protocol, (d) inventory/vars/stats, (e) interfaces/tutorial-visible startup packets, (f) social/chat, and (g) camera/audio/misc. Each section needs source-ordered fields, exact length, state mutation, generation/redraw consequence, and an independent golden.
4. For each partial/missing row, first trace the Java branch in `client.java` and record its fields and side effects. Then compare the matching 274 semantic path. Reuse a 274 handler only if widths, order, signedness/transforms, and lifecycle semantics match; otherwise add an explicit R289 handler. Never translate IDs by position or name alone.
5. Treat conditional script-driven emissions as reachable: inspect `/Users/acfrazier/experiments/lostcity-289/engine` and `/content` for timers, tutorial, hints, interface, and startup branches. A missing live condition is not evidence that a protocol row can be omitted. The isolated engine's `HINT_ARROW` is opcode 115 with an exact six-byte payload; it must be traced and implemented as its own R289 semantic row, not inferred from the 274 constant.
6. Preserve ownership: packet handlers mutate client simulation state and bump the appropriate generation; rendering only observes published state. Preserve `scene_state==1` last-FBO freeze and do not move renderer ownership into packet or script code.

## Bounded implementation sections

- Lifecycle/reset: login revision selection, logout/reset, framing reset, generation invalidation, and strict end-of-frame checks.
- Actors: player/NPC base updates plus every source-defined update-mask field; retain timer/generation behavior and add R289 goldens before enabling.
- World/zone: rebuild, follows, enclosed-zone outer frame, all ten inner zone operations, and redraw/model-stamp ownership.
- Inventory/vars/stats: full/partial/stop-transmit inventory, varp small/large/sync, stats, run energy/weight; verify g2/gsmart and transformed reads.
- Interfaces/tutorial: open/close/modal, text, animation, colour, side/overlay, tab/icon/model/head/scroll/hide operations, tutorial/hint emissions. Exact payload lengths are mandatory.
- Social/chat: filters, game/private/public messages, friend/ignore lists, player options; preserve 274 chat effect semantics under the revision gate.
- Camera/audio/misc: camera operations, map flag/multiway/minimap, reboot/login info, reset animations, MIDI/synth; classify redraw versus side-effect-only behavior.

## Outbound audit

The JSON records all 82 outbound actions and whether a named Rust production path is present. Before the next live proof, verify each reachable action from engine/content script branches, especially startup/tutorial/interface/timer emissions, mouse/event packets, anti-cheat/cyclelogic packets, and `HINT_ARROW`-adjacent actions. Unknown field layout is a blocker, not permission to emit a plausible packet.

## Genuine revision-only gaps

The ledger distinguishes R289-only opcode remapping/lengths, enclosed-zone inner IDs, and script-emitted startup/interface/chat packets from 274 semantic operations that can eventually share a handler. Tutorial/guardian/random-event policy, live credentials/endpoint, cache pairing, and bot action APIs remain outside this cleanup contract. They must be listed as prerequisites or out of scope, never silently treated as complete.
