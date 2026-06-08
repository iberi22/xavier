# GitHub Copilot Instructions

This repository follows **Git-Core Protocol v3.6** with Xavier (port 8006) as the shared memory backend.

## Canonical Read Order

1. `AGENTS.md` — workflow contract and agent instructions (root — industry standard)
2. `SOUL.md` — agent identity/personality
3. `USER.md` — user/human context
4. `.gitcore/ARCHITECTURE.md` — non-negotiable architecture
5. `.gitcore/features.json` — feature status
6. `.gitcore/planning/PLANNING.md` — scope and phase
7. `.gitcore/planning/TASK.md` — current active tasks
8. `README.md` — product entrypoint
9. `docs/agent-docs/RESEARCH_STACK_CONTEXT.md` — for dependency upgrades

## 📌 REPO HYGIENE — MANDATORY (GitCore Protocol)

Canonical reference: `E:\scripts-python\GitCore\.gitcore\STRUCTURE.md`

### ✅ Root — These BELONG at root (industry standard)
- `AGENTS.md`, `SOUL.md`, `USER.md` — agent configs that AI agents expect at root
- `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `LICENSE`, `SECURITY.md`
- `Cargo.toml`, `package.json`, `.gitignore`
- `.github/`, `.cursor/`, `.claude/` — platform-specific config dirs

### ❌ Root — NEVER put these here
- Temp scripts (`fix_*.py`, `fix_*.cjs`, `temp_*.yml`) — go in `scripts/` or delete
- Issue templates — go in `.github/ISSUE_TEMPLATE/`
- Build artifacts (`build-errors-*.txt`, `cargo_check_output.txt`) — delete or gitignore
- Runtime databases — go in `.xavier/` (gitignored via `.xavier/` rule)
- ZIP archives or binary blobs — go in `backup/` or delete
- Task tracking files (`TODO.md`, `PLAN.md`, `TASK_STATE.md`) — use `.gitcore/planning/TASK.md`

### ✅ Where files belong
| File Type | Location |
|-----------|----------|
| Agent configs (AGENTS.md, SOUL.md, USER.md) | **Root** — industry standard for AI agents |
| Architecture/spec docs | `.gitcore/` or `docs/` |
| Task/planning state | `.gitcore/planning/` |
| Issue templates | `.github/ISSUE_TEMPLATE/` |
| Scripts/utilities | `scripts/` |
| Docker variants | `docker/` |
| Xavier runtime state (SQLite DBs) | `.xavier/` (gitignored) |
| Docs/analysis | `docs/` |
| Build output | gitignored (never tracked) |

### 🔍 Before creating any file
Ask: "Does this belong in the root, or is there a canonical subdirectory for it?"
If you're a coding agent (Jules, Cursor, etc.) and you create temp files, **CLEAN THEM UP** after the task completes.

## Memory Model

- **GitHub Issues** — source of truth for task state and progress
- **Xavier** (port 8006, token from env `XAVIER_TOKEN`) — source of truth for reusable project memory, research, and long-horizon agent context
- Do NOT create local tracking files (`TODO.md`, `PLAN.md`, `PROGRESS.md`, workflow `CHANGELOG.md`)
- **Pre-task**: Query Xavier for relevant past decisions
- **Post-task**: Store verified state and devlog in Xavier

## Required Workflow

1. Read canonical files in order (see above)
2. Query Xavier for relevant context (`POST /memory/search`)
3. Follow `.gitcore/ARCHITECTURE.md` if issue presents conflicting stack choices
4. Keep commits atomic, conventional, referencing the issue
5. After completion: store summary in Xavier, clean up temp files

## IDE / MCP Rules

- Xavier MCP at `http://localhost:8006/mcp`
- Keep credentials in machine environment variables only
- Do NOT hardcode access tokens into files
- Never commit `vec-store.sqlite3*` or any `.db-wal`/`.db-shm` files

## Cartography (`.gitignore` Integrity)

The `.gitignore` was repaired on 2026-06-08 — the final line was encoded in UTF-16 LE (corrupt). Ensure any edits to `.gitignore` stay in **UTF-8 without BOM**. If you notice encoding corruption, report it immediately.
