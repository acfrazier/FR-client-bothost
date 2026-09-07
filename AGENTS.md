# Revision 289 client campaign

This checkout is /Users/acfrazier/experiments/FR-client-289, branch
codex/revision-289-client, in acfrazier/FR-client-bothost. Read
`docs/revision-289/STATE.md`, then its named current plan section.

The operator explicitly authorized this separate campaign on 2026-09-07 while
274bot memory work continues. This supersedes memory-first sequencing only for
this isolated client campaign. Deliver a bothost-capable 289 client while
preserving the working 274 path and client-play. Rust owns client protocol,
cache, rendering and lifecycle; do not add a bot action API inside client.

Apply the operator's existing Git and profile/review rules from
/Users/acfrazier/experiments/274bot/.worktrees/t_a1f4796f/AGENTS.md and execution
workflow from that checkout's docs/execution.md. Read those two files once;
do not import the host's state/history or memory campaign task list. Use the
existing Hermes 274bot board with 289-prefixed cards and exact client workspace.
The assigned Astra orch may dispatch implementer/luna/reviewer/branchreviewer
by profile defaults and must follow through actual review completion.

Repo hygiene (remotes, merges, pushes and submodules) belongs to the parent
Codex orchestrator. No worker or spawned orch may push/merge/change remotes.
Stay on the prepared branch; no modifications to any 274bot checkout, its
submodule, shared build directories, measurement fixtures or live hosts.
Use CARGO_TARGET_DIR=/Users/acfrazier/experiments/FR-client-289/target.
Source/vault/server references outside this checkout are read-only. Do not copy
private server/vault material into public commit history; create independently
justified minimal protocol fixtures with recorded provenance. Coordinate any
new live server or remote machine use with the parent; offline client work is
authorized. Missing live prerequisites do not prevent useful offline stages.

Preserve existing GPU/CPU rendering ownership and scene_state==1 last-FBO
freeze. Read primary 289 Java before numeric protocol/config changes. Keep
packet/lifecycle/config proof distinct from compilation and type compatibility.
Client integration tests run in this client workspace, independently of host
tests. Required final Grok4.6 review is not replaced by per-task review.

Maintain docs/revision-289/STATE.md with active card IDs, accepted commits,
exact tests, missing prerequisites and next steps. No usage reset redemption.
