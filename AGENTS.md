# AGENTS.md - Xavier Workspace

## 📖 ORDEN DE LECTURA (Reading Order)

> **⚠️ Todos los agentes deben seguir este orden al iniciar una sesión.**

| Paso | Archivo | Propósito |
|------|---------|-----------|
| 1 | `SOUL.md` | Quién es Xavier — identidad, valores, personalidad |
| 2 | `USER.md` | Quién es BELA — contexto del humano a cargo |
| 3 | `MEMORY.md` | Memoria persistente — decisiones y lecciones pasadas |
| 4 | `RULES.md` | Reglas de codificación, Rust, agentes, documentación |
| 5 | `.gitcore/planning/PLANNING.md` | Visión del proyecto, fases, prioridades Q3 2026 |
| 6 | `.gitcore/planning/TASK.md` | Tareas activas, progreso por componente, deuda técnica |
| 7 | `.gitcore/features.json` | Estado de 20 features con tests y validación |
| 8 | `.gitcore/SRC.md` | Estructura del código fuente y entry points |
| 9 | `docs/devlog/` (último) | Bitácora técnica de la semana actual |

**Regla de oro:** Siempre busca en Xavier (`POST /memory/search`) antes de empezar una tarea compleja. Siempre persiste después de completar.

---

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
- **Persist**: document your decision/finding in the PR description so it can be back-filled
  into Xavier by the dispatcher after merge.
- If you have network access to `http://localhost:8006` in your sandbox, you MAY also call
  `POST /memory/search` directly (header `X-Xavier-Token` from the issue context).

### Dispatcher
`scripts/subagents/dispatch.py` is the single entry point for routing a task to the right
subagent type (`agi` for sync, `jules` for async). It handles Recall → Dispatch → Persist
automatically. See `scripts/subagents/xavier_brain_prompt.md` for the full protocol.

## Best Practices & Performance
- **Golden Rule (Tokio + Rayon)**: When combining both, never call Rayon's `.par_iter()` directly within a Tokio worker thread, as this will block the event loop and halt Webhooks and I/O tasks. Always wrap Rayon-based computation inside `tokio::task::spawn_blocking`. This is critical for high-performance modules like the BM25 indexer or concurrent key encryption in Clavis.

## Main Project
- Repo: `iberi22/xavier` — Open source context engine.
- Stack: Rust + SQLite-Vec.
- Plugins: PgHeart ("~/dev/pgheart").
- Objective: To become the central memory system for all SWAL agents.

---

_Last updated: 2026-05-13_
