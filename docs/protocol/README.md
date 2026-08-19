# Development Protocol

This directory documents **how Xavier is developed** — the wave-based engineering
protocol, the feature lifecycle, and the contribution rules. It is the public
face of the GitCore protocol used across the SWAL ecosystem.

> For humans: this is the "how to work on this repo" guide.
> For AI agents: read `AGENTS.md` first, then come here for the deep protocol.

## Why a protocol?

Xavier is built in **waves** (sprints) with a **verifiable feature ledger**
(`features.json`): every feature declares its own tests, and the verification
pipeline re-checks them. Nothing is "done" by declaration — only by a green run.

## Map

| Path | Purpose |
|------|---------|
| `PROTOCOL_REFERENCE.md` | The wave protocol: research → issues → execution → verification |
| `SDLC_WORKFLOW.md` | The full SDLC loop (spec → plan → build → verify → ship) |
| `CLI_CONFIG.md` | Tooling dependency graph and capability map |
| `rules/` | Agent integration rules (how agents use Xavier's memory) |
| `../features/README.md` | The feature ledger contract (`features.json`) |
| `../features/features.json` | Source of truth: every feature, its status and tests |
| `../features/specs/` | One spec per feature (FEATURE-*.md) |
| `../../scripts/verify-pipeline.sh` | The verification harness (run it, don't trust) |

## Quick start for a contributor

1. Read `../../AGENTS.md` — the project contract for agents and humans.
2. Read `../features/README.md` — how features are tracked and verified.
3. Run `../../scripts/verify-pipeline.sh` — see the real state of the ledger.
4. Open an issue using the templates in `../../.github/ISSUE_TEMPLATE/`.
5. Propose a change: 1 PR = 1 feature, referencing its `F-XXX` id.
