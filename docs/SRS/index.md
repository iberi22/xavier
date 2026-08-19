# xavier — Software Requirements Specification (SRS)

> **Protocol:** GitCore 3.8.0
> **Updated:** 2026-08-04
> **Status target:** structure complete · 19 REQ-IDs · 32 user stories · features at honest 85.2%

## Current status

| Metric | Value |
|--------|-------|
| Total requirements | 19 (REQ-001…019) |
| Structure complete | ✅ 100% |
| User stories | 32 (US-001…032), traceable to REQ + feature |
| Features (features.json) | 27 — overall **85.2%** (23 stable, 4 beta) |
| Content status | REQ-008 `implemented` ~95% (E2E+unit) |
| Pipeline | `.gitcore/scripts/verify-pipeline.sh` — 27/27 PASS (2026-08-04) |
| Synced ratio (drift) | n/a (local); pipeline enforces REQ↔feature↔story links |

## Documents (mandatory)

| Doc | Purpose |
|-----|---------|
| [REQUIREMENTS.md](./REQUIREMENTS.md) | REQ-IDs 001–019, acceptance criteria, file traces |
| [USER-STORIES.md](./USER-STORIES.md) | US-001–032, `As a <role>…` format, REQ/feature links |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Component map, constraints, SWAL alignment |

## Key requirements

| REQ | Topic | Features | Status |
|-----|-------|----------|--------|
| REQ-001/002 | Protocol + SRC compliance | feat-src-reference, feat-documentation-site | verified |
| REQ-003/008 | SWAL node Pro gate + decentralized login | feat-decentralized-login | implemented 95% |
| REQ-005/009/010 | Memory, hybrid search, MCP/HTTP | feat-unified-storage, feat-hybrid-search, feat-mcp-server | verified |
| REQ-011 | Code graph + plugins + explorer | feat-code-graph-index, feat-plugin-system (70%), feat-graph-explorer | implemented |
| REQ-012 | Mesh P2P | feat-mesh-network (**45%**, EPIC #115) | draft |
| REQ-013 | Notifications + Telegram | feat-notification-system, feat-telegram-bot (60%) | implemented |
| REQ-014 | Governance DAO | feat-governance-dao (90%) | implemented |
| REQ-015/016/017 | Health, context regen, local-first | feat-runtime-health, feat-context-regeneration, feat-local-first | verified |
| REQ-018/019 | License, agent tooling | feat-dual-license, feat-openclaw-scanner, feat-agent-cli-commands | verified |

## Login / identity (REQ-008)

| Resource | Path |
|----------|------|
| Feature | `.gitcore/features/FEATURE-feat-decentralized-login.md` (**95%**) |
| Issues + % | `.gitcore/issues/login/PROGRESS.md` |
| Test evidence | `.gitcore/issues/login/TEST_EVIDENCE.md` |
| E2E | `tests/e2e/decentralized_login_e2e.rs` (5 PASS) |
| Session | `.gitcore/docs/SESSION_LOGIN_2026-07-28.md` |

## Optional (domain)

- `NON-FUNCTIONAL.md` — performance, security, privacy
- `INTERFACES.md` — APIs, MCP, mesh messages
- `DATABASE.md` — schema / storage of **business** data (not Xavier paths)

## Rules

1. Every feature in `.gitcore/features.json` maps to ≥1 REQ-ID **and** ≥1 US (enforced by `verify-pipeline.sh`).
2. Pro/subscription REQs reference **SWAL node**, never Stripe.
3. Multi-instance data isolation: `app_id` + `instance_id`.
4. Agentic memory requirements point to **Xavier** (HTTP/MCP), outside business DB.
5. `progress_pct` must be backed by real paths, real tests, honest notes — pipeline FAIL=0 before merge.

## Pipeline (local)

| Phase | Tool | Notes |
|-------|------|-------|
| Feature reality | `.gitcore/scripts/verify-pipeline.sh` | paths + REQ + story + tests (fast) |
| + compile | `verify-pipeline.sh --check` | cargo check gate |
| + tests | `verify-pipeline.sh --test` | cargo test gate |
| Machine report | `verify-pipeline.sh --json` | CI-ready JSON |
| Drift | `GitCore/scripts/drift-detector.py` | Optional local |

## Cross-links

- [SRC.md](../../SRC.md) — repository map
- [AGENTS.md](../../AGENTS.md) — agent rules
- [SDLC_WORKFLOW.md](../../.gitcore/SDLC_WORKFLOW.md) — full SDLC with traceability chain
- [SWAL roadmap](../../../docs/SWAL/README.md) — ecosystem (if monorepo)
