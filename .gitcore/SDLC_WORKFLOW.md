# SDLC Workflow — xavier

**Version:** 2.0
**Date:** 2026-08-04
**Protocol:** GitCore 3.8.0 (REQ-001, REQ-007)
**Project:** Xavier Cognitive Memory System

---

## 1. Overview

This document defines the Software Development Life Cycle for Xavier, wired to the
GitCore protocol and enforced by automated verification (`verify-pipeline.sh`).
Every feature is traceable: **User Story (US-NNN) → SRS Requirement (REQ-NNN) →
GitHub Issue → Implementation → Tests → Verified % in features.json**.

No feature ships without passing the pipeline. No feature claims a % that the
pipeline cannot back with real paths, real SRS links, and real test references.

---

## 2. Tech Stack

| Component | Technology |
|-----------|------------|
| Runtime | Rust + Tokio |
| HTTP Server | Axum |
| Memory Backend | SQLite + SQLite-vec |
| MCP | Model Context Protocol (streamable HTTP :8100) |
| Code Index | code-graph sidecar crate |
| UI | React + Vite (panel-ui) |
| Docs | Astro (Starlight) + SRS/SRC per GitCore |
| Verification | `.gitcore/scripts/verify-pipeline.sh` |

---

## 3. SDLC Flow (Traceability Chain)

```
USER STORY (US-NNN) ──> SRS REQ (REQ-NNN) ──> GITHUB ISSUE ──> BRANCH
     │                        │                    │              │
     │  docs/SRS/USER-STORIES │  docs/SRS/REQUIREMENTS.md          │
     │                        │                    │              ▼
     │                        │                    │         IMPLEMENT (src/)
     │                        │                    │              │
     │                        │                    │              ▼
     │                        │                    │         TESTS (cargo test)
     │                        │                    │              │
     └──── verify-pipeline.sh ◄─── REQ/US present ◄──┘              │
          (paths + SRS + stories + tests + compile)                │
                    │                                              │
                    ▼                                              ▼
          features.json % updated ◄───────────── MERGE (main)
                    │
                    ▼
          Post-merge: pipeline re-run → % locked
```

### Rules

1. **Every issue links ≥1 REQ-NNN and ≥1 US-NNN** in its body (REQ-001 traceability).
2. **features.json is updated in the SAME PR as the code** — never in a separate
   "reconciliation" issue that inflates % without evidence.
3. **% = real evidence only**: `progress_pct` must be backed by implemented_in paths
   that exist, tests that exist, and honest notes (no "MVP"/"Phase 1" claims at 100%).
4. **verify-pipeline.sh must PASS before merge** (`bash .gitcore/scripts/verify-pipeline.sh --check`).
5. **Mesh/XenBench/WASM features stay <100%** until Phase 2+ ships (see REQ-012).

---

## 4. Phases

### Phase 1: Analysis (Requirements)

| Step | Action | Command |
|------|--------|---------|
| 1.1 | Create user story | Add to `docs/SRS/USER-STORIES.md` (US-NNN) |
| 1.2 | Create/update SRS requirement | Add to `docs/SRS/REQUIREMENTS.md` (REQ-NNN) |
| 1.3 | Create issue with traceability | `gh issue create --title "..." --body-file body.md` (body includes REQ + US) |
| 1.4 | Add labels | `gh issue edit N --add-label feat,jules` |

### Phase 2: Implementation

| Step | Action | Command |
|------|--------|---------|
| 2.1 | Create branch | `git checkout -b feat/description-#N` |
| 2.2 | Implement | Rust code in `src/` (+ update `implemented_in`) |
| 2.3 | Unit tests | `cargo test -p xavier --lib` |
| 2.4 | Lint | `cargo clippy -p xavier --all-targets -- -D warnings` (0 errors) |
| 2.5 | Update features.json in-PR | Set `progress_pct`, `last_tested`, real `tests` refs |

### Phase 3: Verification (Automated Gate)

| Step | Action | Command |
|------|--------|---------|
| 3.1 | Reality pipeline | `bash .gitcore/scripts/verify-pipeline.sh` |
| 3.2 | + compile | `bash .gitcore/scripts/verify-pipeline.sh --check` |
| 3.3 | + tests | `bash .gitcore/scripts/verify-pipeline.sh --test` |
| 3.4 | Machine report | `bash .gitcore/scripts/verify-pipeline.sh --json` |

**Gate: FAIL=0 required.** Any `MISSING_PATH`, `NO_REQ`, `NO_STORY`, or `NO_TEST_REF`
must be resolved before merge.

### Phase 4: Review

| Step | Action |
|------|--------|
| 4.1 | Code review via PR (gh pr review) |
| 4.2 | Verify pipeline PASS in PR comment |
| 4.3 | Verify clippy 0 errors |
| 4.4 | Merge (squash, delete branch) |

### Phase 5: Deploy & Close

| Step | Action | Command |
|------|--------|---------|
| 5.1 | Merge to main | `gh pr merge N --squash --delete-branch` |
| 5.2 | Re-run pipeline | `bash .gitcore/scripts/verify-pipeline.sh` |
| 5.3 | Close issue with evidence | `gh issue close N --comment "..."` |
| 5.4 | Update CHANGELOG.md | Add entry with feature + REQ + US |

---

## 5. Verification Pipeline Details

`verify-pipeline.sh` runs 6 checks per feature:

| # | Check | Source | Failure flag |
|---|-------|--------|--------------|
| 1 | `implemented_in` paths exist on disk | features.json | `MISSING_PATH` |
| 2 | `req_ids` exist in REQUIREMENTS.md | docs/SRS/ | `NO_REQ` |
| 3 | `user_stories` exist in USER-STORIES.md | docs/SRS/ | `NO_STORY` |
| 4 | `tests` refs exist in source (final symbol) | src/, tests/, code-graph/ | `NO_TEST_REF` |
| 5 | cargo check passes (--check) | cargo | check status |
| 6 | cargo test passes (--test) | cargo | test status |

Output: per-feature PASS/FAIL + summary + optional JSON. Exit 0 = all pass.

---

## 6. Commit Conventions

```
type(scope): description #issue

Types: feat | fix | docs | refactor | test | chore
Scopes: memory | server | agents | mesh | security | code-graph | panel | docs

Examples:
feat(memory): add hybrid search RRF fusion #45
fix(server): resolve token validation timeout #47
docs(srs): add REQ-012 mesh phases with honest 45% #115
```

---

## 7. Issue Labels

| Label | Use |
|-------|-----|
| `feat` | New feature (must link REQ + US) |
| `bug` | Bug report |
| `docs` | Documentation |
| `refactor` | Refactor |
| `test` | Tests |
| `jules` | Dispatch to Jules (async agent) |
| `priority-high` | High priority |

---

## 8. Feature Status Tracking

- **Canonical:** `.gitcore/features.json` (27 features, `req_ids`, `user_stories`,
  `progress_pct`, `last_tested`, real `tests`).
- **Detailed:** `.gitcore/features-detailed.json` (sub_features per phase).
- **Current reality (2026-08-04):** overall **85.2%** — 23 stable, 4 beta
  (mesh-network 45%, telegram-bot 60%, auto-improvement 55%, plugin-system 70%).
- **SRS:** `docs/SRS/REQUIREMENTS.md` — REQ-001..REQ-019 (English, acceptance criteria).
- **Stories:** `docs/SRS/USER-STORIES.md` — US-001..US-032 (traceable to REQ + feat).

---

## 9. Testing Commands

```bash
# Reality pipeline (fast)
bash .gitcore/scripts/verify-pipeline.sh

# + compile gate
bash .gitcore/scripts/verify-pipeline.sh --check

# + full tests
bash .gitcore/scripts/verify-pipeline.sh --test

# Unit tests
cargo test -p xavier --lib

# All tests
cargo test -p xavier

# E2E
cargo test -p xavier --test e2e -- --nocapture

# Lint (0 warnings required)
cargo clippy -p xavier --all-targets -- -D warnings

# Format
cargo fmt --check
```

---

*Xavier v0.12.0 — SDLC Workflow v2.0 (GitCore 3.8.0). Updated 2026-08-04.*
