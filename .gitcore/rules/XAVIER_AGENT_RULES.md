# 🤖 Xavier Core Integration Guidelines for Coding Agents
> **Mandatory System Directive**: This document defines the protocol for AI agents to interact with Xavier.

---

## 🧭 The Core Principle: "Recall, Analyze, Persist"

```
┌────────────────────────────────┐
│   1. RECALL (Pre-Task)         │ ──▶ Query Xavier Memory for historical decisions
└────────────────────────────────┘
               │
               ▼
┌────────────────────────────────┐
│   2. ANALYZE (Static Graph)    │ ──▶ Map symbols & dependencies via /code/
└────────────────────────────────┘
               │
               ▼
┌────────────────────────────────┐
│   3. PERSIST (Post-Task)       │ ──▶ Store verified state and devlog in memory
└────────────────────────────────┘
```

## 📡 Endpoints

All HTTP requests to Xavier target `http://localhost:8006` with header `X-Xavier-Token: <env XAVIER_TOKEN>`.

## ⚠️ REPO HYGIENE RULES (GitCore Protocol)

These rules are **MANDATORY** for all agents working in this repo:

### Files at root — what belongs vs what doesn't

#### ✅ BELONGS at root (industry standard for AI agents)
- `AGENTS.md`, `SOUL.md`, `USER.md` — AI agent configs (Cursor, Claude, Copilot, OpenClaw expect these at root)
- `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `LICENSE`, `SECURITY.md` — standard project docs
- `Cargo.toml`, `package.json`, `.gitignore` — language/tool configs
- `.github/`, `.cursor/`, `.claude/` — platform-specific directories

#### ❌ NEVER at root
- Temporary scripts (`fix_*.py`, `fix_*.cjs`, `temp_*.yml`) → go in `scripts/` or clean up after task
- Issue templates → go in `.github/ISSUE_TEMPLATE/`
- Build artifacts → gitignore or delete
- Runtime SQLite databases → go in `.xavier/` (already gitignored via `.xavier/` rule)
- Task tracking files → use `.gitcore/planning/TASK.md`
- ZIP archives or binary blobs → go in `backup/` or delete

### The 3-Question Test before creating any file:
1. Does this file already have a canonical location in the repo structure?
2. Is this a temp file I'll delete after the task?
3. Would this file make the root directory look messy?

If yes to any of #1 or #3, put it in the right place. If #2, set a reminder to clean up.

### .gitignore Integrity
- The `.gitignore` was repaired on 2026-06-08 (was UTF-16 LE corrupted on last line)
- Always write `.gitignore` in **UTF-8 without BOM**
- If a file isn't being ignored when it should be, check for encoding corruption

---

## 🧠 Memory Operations

### Pre-Task: Semantic RAG
- **Endpoint**: `POST /memory/search`
- **Query**: include relevant keywords for your task context

### Post-Task: Store Summary
- **Endpoint**: `POST /memory/add`
- **Path**: `tasks/verification/<issue_number_or_slug>`
- **Content**: markdown summary of changes, why, and verification status

## ⚠️ Constraints
1. **Mandatory Token Header**: Never omit `X-Xavier-Token`
2. **No Secrets**: Never store API keys or tokens in Xavier memory
3. **No Redundant RAG**: Use code graph for code lookups, vector memory only for docs/specs/task summaries
4. **Cleanup after yourself**: Delete temp files created during the task
