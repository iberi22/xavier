# AGENTS.md - Xavier Workspace

## Identity
Xavier is the **CEO of the SWAL project** alongside BELA. It is the central system for memory and continuous improvement.

## Essential Files (Read at the start of every session)
1. `SOUL.md` — Who Xavier is
2. `USER.md` — Who BELA is
3. `MEMORY.md` — Long-term memory
4. `memory/YYYY-MM-DD.md` — Daily logs and notes

## Core Memory (Xavier Core)
Xavier is the global memory brain. **Cortex** (previously the synchronization plugin) has been fully removed:
|-**Xavier URL:** http://localhost:8006
|- Durable Memory: Always search Xavier (`http://localhost:8006`) for past context BEFORE starting complex tasks. See [.gitcore/rules/GLOBAL_XAVIER_INTEGRATION.md](file:///e:/scripts-python/xavier-v1/.gitcore/rules/GLOBAL_XAVIER_INTEGRATION.md) and [.gitcore/rules/XAVIER_AGENT_RULES.md](file:///e:/scripts-python/xavier-v1/.gitcore/rules/XAVIER_AGENT_RULES.md) for concrete guidelines.
- **Cascade Integration**: Integrate Xavier into every turn of the agentic flow for turn-based context and atomic verification.
- **Durable Learning**: Store deep research findings or architectural decisions in Xavier after task completion.
- **Roadmap Management** — Manage and update the project roadmap.
- **Continuous Improvement** — Identify opportunities for enhancement.
- **Coordination** — Ensure all agents are aligned.
- **Strategic Decisions** — Make architectural and priority decisions.
- **DevLog Management** — Document the deep technical "why". See `docs/devlog/`.

## 🧠 Subagents: Xavier as the shared brain (MANDATORY protocol)

This repo is worked on by **two kinds of subagent**. ALL of them MUST treat Xavier
(`http://localhost:8006`, or the MCP server `xavier-memory`) as their **sole durable memory**.
Your own context is discarded between sessions — only what you store in Xavier persists.

### 1. AGI CLIs (synchronous) — codex / opencode / gemini / claude / qwen
Launched locally with Xavier wired as an MCP server (see `scripts/subagents/mcp/`).
- **Recall**: call `mem_search(query=<your task>, filters={project:<your-project>})` BEFORE working.
- **Persist**: call `create_memory(path=<descriptive-slug>, content=<result>, kind=<decision|fact|task|bug>)` AFTER.

### 2. Google Jules (asynchronous)
Triggered by applying the `jules` label to a GitHub issue. Runs in its own sandbox and
opens a Pull Request. Jules reads this `AGENTS.md` first, so:
- **Recall**: read the "Contexto recuperado de Xavier" block injected in the issue body
  (the dispatcher puts it there — use it, do not re-discover what is already decided).
- **Persist**: document your decision/finding in the PR description so it can be back-filled.

- **Progressive Memory Disclosure**: To save tokens, **ALWAYS** use `mem_search` (Fat Search) first to identify relevant memories via metadata and snippets. Only use `memory_context` or `get_memory` (Page-In) for the specific IDs or paths you need to see in full.

## Best Practices & Performance
- **Golden Rule (Tokio + Rayon)**: When combining both, never call Rayon's `.par_iter()` directly within a Tokio worker thread, as this will block the event loop and halt Webhooks and I/O tasks. Always wrap Rayon-based computation inside `tokio::task::spawn_blocking`. This is critical for high-performance modules like the BM25 indexer or concurrent key encryption in Clavis.

## 🌍 Entorno de Xavier (Environment Detection)
Xavier checks the following environment variables at startup to configure paths and behaviors:

### `XAVIER_HOME` (optional)
The workspace directory where Xavier stores configuration and data beyond the SQLite database.
- If `$XAVIER_HOME` is set, Xavier uses it directly.
- Otherwise, Xavier walks up the file tree from the current directory looking for a `.xavier-root` file or `.git` directory and places `XAVIER_HOME` at the repo root under `.xavier/`.
- This directory stores non-DB artifacts: cron logs, lock files, temporary states.

### `XAVIER_CRON_SLEEP_MINUTES` (optional)
How many minutes to wait between cron cycles.
- Default: `1` (one minute).
- Values below `1` are clamped to `1`; values above `60` are clamped to `60`

### `XAVIER_WORKTREE` (internal use)
Used by the `worktree` command to delegate a full copy of Xavier to a subagent worktree.
- Path to a full Xavier worktree copy.

---

## GitHub Integration
- Repo: `iberi22/xavier` — Open source context engine.
- Stack: Rust + SQLite-Vec.
- Plugins: PgHeart ("~/dev/pgheart").
- Objective: To become the central memory system for all SWAL agents.

---

_Last updated: 2026-05-13_
