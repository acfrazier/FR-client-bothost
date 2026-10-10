# Docs

This tree is the **274bot** client fork (default branch
`multirev-bh-modular`, renamed from `r274-bh-modular` on 2026-10-10). Host
product docs live in [274bot](https://github.com/acfrazier/274bot):
[`README.md`](https://github.com/acfrazier/274bot/blob/main/README.md),
[`docs/api/`](https://github.com/acfrazier/274bot/blob/main/docs/api/README.md).

In this repo: [README.md](../README.md), [NOTICE.md](../NOTICE.md),
[`crates/client-play/README.md`](../crates/client-play/README.md).

## Reference (`revision-289/`)

- [`protocol-289.json`](revision-289/protocol-289.json) — 289 inbound (256)
  and outbound (82) tables with source anchors; `tools/` generators and the
  `SERVER_PROT_SIZES_289` include build from it. Known drift 2026-10-10:
  generated include row 158 is 4, the JSON (Class17-table value) says 0 —
  regen pending, JSON stands.
- [`source-contract.md`](revision-289/source-contract.md) — source-grounded
  framing, login/RSA, inbound/outbound contract and Rust module map.
- [`implementation.md`](revision-289/implementation.md) — architecture
  decisions and production bind points for the 289 stages (file:line anchors
  predate later growth; see its header note).
- [`full-dispatch-audit.json`](revision-289/full-dispatch-audit.json) —
  70-operation source ledger (fields, Java branches, generation families),
  frozen at audited commit `a97b4ae`.

The 289 campaign's process records (STATE, reviews, fix reports, evidence
captures) were moved out of the tree on 2026-10-10; git history keeps them.
