# FEATURE: Autonomous Delivery Pipeline (verifiable ship/show/ask + DONE)

**Status:** `planned`
**Ledger feature ID:** `feat-autonomous-delivery`

## Overview

This feature registers the durable contract for Xavier's autonomous delivery: an
evidence-gated pipeline in which `DONE` is a computed verdict for an immutable SHA, never a
checkbox or agent assertion. There is no code PR in this task; this task records the spec,
acceptance scenarios, SRS mapping (`REQ-085`) and the `status: planned` ledger entry, per the
issue's Desired State. Status may only change on a green pipeline run — it is never
hand-promoted (`AGENTS.md` §4-§5; issue >= 2652).

The authoritative design lives in-repo and is cited, not restated:

- Design: `docs/design/trunk-based/01-DESIGN.md` (flow + Ship/Show/Ask classification).
- Pipeline stages and gates: `docs/design/trunk-based/02-AUTONOMOUS-PIPELINE.md`.
- DONE protocol (verifier contract): `docs/design/trunk-based/03-DONE-PROTOCOL.md`.
- Policy decision: `docs/adr/ADR-035-trunk-based-ship-show-ask.md`.

## Core mechanics (from the cited design)

- **Computed verdict.** `NOT_RUN`, `SKIPPED`, `TIMED_OUT`, `CANCELLED`, missing/stale
  evidence and reviewer `PARTIAL` all fail required gates; a new commit invalidates the
  verdict (03-DONE-PROTOCOL.md).
- **Per-stage evidence chain.** Each of the 8 stages writes an accepted evidence record tied
  to `session_id`, feature/task ID, base/head SHA, timestamp, backend and artifact/URL; the
  next stage reads the previous stage's record, not the agent's prose
  (02-AUTONOMOUS-PIPELINE.md).
- **Review-timing classification.** `flow:ship`/`flow:show`/`flow:ask` is the *minimum* class
  computed from the change's paths and diff under 01-DESIGN.md; the label controls review
  timing, never verification strength, and Ask is the default for unknown or mixed paths.
- **Independent decision rights.** Two independent models, neither the author, approve the
  Ask decision/ADR/task plan; rollback reverts and outward-publishing changes always need
  guardian evidence plus pre-merge panel review and never auto-merge (02-AUTONOMOUS-PIPELINE.md).
- **Evidence bundle.** One JSON bundle per candidate (schema fields: `schema_version`,
  commands/ci_runs/reviews with SHAs and exit/test counts, artifacts, allowed/changed paths,
  risk, rollback, ...); the DONE verifier recomputes scope from git, scans secrets, re-runs
  declared commands and checks CI/Atlas/review, and never edits the ledger (03-DONE-PROTOCOL.md).
- **Ledger is separate.** Only `scripts/verify-pipeline.sh` governs promotion after executed
  tests; `scripts/done-check.sh` is the future verifier (not yet present) defined by
  03-DONE-PROTOCOL.md.

## Acceptance scenarios

1. **Missing/stale evidence blocks advance.** A stage whose required predecessor evidence
   record is absent, stale, or SHA-mismatched cannot advance.
2. **Class computed, not chosen.** A diff touching `docs/SRS/**` or an unknown/mixed path
   classifies as Ask, and downgrade from a stricter computed class is not a green check.
3. **Cross-model review required for Ask.** A PR without two independent model approvals
   (neither the author) fails the Ask/ADR/task-plan gate.
4. **Rollback can never auto-merge.** A rollback revert PR without guardian evidence and
   pre-merge panel review fails, regardless of its ship/show/ask class.
5. **DONE verdict is computed.** `done-check.sh` for a candidate passes only when the
   evidence bundle recomputes and all checks align at one immutable SHA; it never edits the
   ledger or its own status.
6. **No hand promotion.** Ledger `status` changes only from a green
   `scripts/verify-pipeline.sh` run after executed tests; declared checks report location
   and kind, never secret values.

## SRS / REQ mapping

- `REQ-085` (docs/SRS/REQUIREMENTS.md) — Autonomous Delivery Pipeline. This feature carries
  `req_ids: ["REQ-085"]` in `.gitcore/features.json`; REQ-085 lists this feature back.

## Scope boundaries (DO NOT touch)

- Enforcing decision rights (branch rules, ruleset activation, public tags/releases,
  paid capacity, production data migrations) is prepared for the owner's Product Acceptance
  Package and never activated silently; branch-rule activation is a separate Phase-3 gate.
- This task touches at most three files; no runtime daemon, database, service or port is
  added (the pipeline reuses the existing process and repositories, per 02-AUTONOMOUS-PIPELINE.md).

## Known Issues & Notes

- ADR-035 is accepted (agent panel, 2026-09-27); this feature records the pipeline
  contract, it does not activate the policy (branch rules stay an owner decision).
- `scripts/done-check.sh` is a defined future contract (03-DONE-PROTOCOL.md); it is cited,
  not assumed to exist.
