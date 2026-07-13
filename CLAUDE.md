# CLAUDE.md — Xavier Project Instructions for Agents

> Este archivo define cómo trabajar en Xavier. Léelo al inicio de cada sesión.
> This file defines how to work on Xavier. Read it at the start of every session.

---

## 📋 Essential Reading Order (all sessions)

| # | File | Purpose |
|---|------|---------|
| 1 | `SOUL.md` | Xavier's identity — CEO of SWAL |
| 2 | `USER.md` | Who BELA is — the human |
| 3 | `AGENTS.md` | Agent protocol + subagent coordination |
| 4 | `CLAUDECODE_TASK.md` | Active task for this session |
| 5 | `.gitcore/features.json` | Feature status tracking |

---

## 🧠 Xavier Memory Protocol

Xavier **is** the memory system. Every agent MUST use it for persistence.

### Server
- **URL:** `http://localhost:8006`
- **Health:** `curl http://localhost:8006/health`
- **Auth header:** `X-Xavier-Token: <token>` (from `XAVIER_TOKEN` env or `.env`)



### Environment Detection
Xavier can run in various environments. Detect the current environment with:
```bash
# Returns: wsl | docker | windows-native | not-running
bash /home/belal/.hermes/scripts/which-xavier.sh
```

The MCP bridge (`xavier-memory`) exposes an `xavier_env()` tool that reports:
- **environment**: wsl / docker / windows-native
- **url**: Xavier's current URL
- **version**: Xavier build version
- **token_status**: configured / missing
- **code_graph_available**: true / false

### Clean Startup
```bash
bash /home/belal/.hermes/scripts/start-xavier.sh
```

### Key Endpoints
```bash
# Add memory
curl -X POST http://localhost:8006/memory/add \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"...", "path":"decisions/my-decision"}'

# Search memory (always do this before complex work)
curl -X POST http://localhost:8006/v1/memories/search \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query":"what we are looking for", "limit":5}'

# Get stats
curl http://localhost:8006/memory/stats \
  -H "X-Xavier-Token: $XAVIER_TOKEN"
```

### MCP Server
Run `xavier mcp` for stdio-based MCP, or connect via the config in `.mcp.json`.

**Protocol:**
1. **SEARCH** memory BEFORE starting complex tasks → `mem_search(query, filters)`
2. **SAVE** results/decisions AFTER completing → `create_memory(path, content, kind)`
3. **UPDATE** `AGENTS.md` and `MEMORY.md` when architecture decisions change

---

## 🦀 Rust Development

### Stack
- **Language:** Rust (edition 2021)
- **Runtime:** Tokio (async)
- **HTTP:** Axum 0.8
- **Database:** SQLite + SQLite-Vec (vectors) + FTS5 (BM25)
- **CLI:** clap
- **Serialization:** serde + serde_json

### Golden Rule (Tokio + Rayon)
Never call Rayon's `.par_iter()` directly within a Tokio worker thread — it blocks the event loop.
Always wrap Rayon computation inside `tokio::task::spawn_blocking`.

### Commands
```bash
# Check
cargo check --workspace

# Test (all)
cargo test --tests -- --test-threads=4

# Test (lib only, fast)
cargo test --lib

# Lint
cargo clippy --workspace -- -D warnings

# Build
cargo build --release

# Run server
cargo run -- http 8006
```

### Code Style
- Follow Rust 2021 idioms
- Modules in `src/` follow domain structure: `memory/`, `server/`, `mesh/`, `security/`, `cli/`, `telegram/`, etc.
- Use `thiserror` for error types
- Use `tracing` for logging (not `log`)
- Keep functions < 50 lines where possible
- Tests live next to code (unit) and in `tests/` (integration)

---

## 🤖 Subagent Coordination

### AGI Agents (codex / opencode / gemini / claude / qwen)
- Wired via MCP to Xavier
- Must call `mem_search` before starting work
- Must persist results via `create_memory`
- Run `scripts/subagents/xavier_brain_prompt.md` for full protocol

### Google Jules (async)
- Triggered by `jules` label on GitHub issues
- Reads context from issue body (injected by dispatcher)
- Documents decisions in PR description

### Dispatcher
`scripts/subagents/dispatch.py` routes tasks to the right agent type.

---

## 📁 Project Structure (key paths)

```
xavier/
├── src/                    # Rust source
│   ├── memory/             # Memory engine, entity graph, HORMER
│   ├── server/             # HTTP, MCP, routes
│   ├── mesh/               # P2P sync, governance
│   ├── security/           # Crypto, auth, license
│   ├── cli/                # CLI commands & handlers
│   ├── telegram/           # Telegram bot
│   └── observability/      # Health, notifications
├── tests/                  # Integration tests
├── scripts/                # Automation, subagents, benchmarks
├── panel-ui/               # Tauri desktop UI (TypeScript/React)
├── docs/                   # Documentation
├── code-graph/             # Code graph sidecar crate
└── .gitcore/               # Git-Core protocol config
```

---

## ✅ Session Checklist

- [ ] Read `SOUL.md`, `USER.md`, `AGENTS.md`
- [ ] Read `CLAUDECODE_TASK.md` for current task
- [ ] Verify Xavier server is running: `curl http://localhost:8006/health`
- [ ] Search Xavier memory for relevant context
- [ ] Work on task
- [ ] Persist results/decisions to Xavier
- [ ] Update task file if needed

---

## 🔗 Quick Links

- **Repo:** https://github.com/iberi22/xavier
- **Issues:** https://github.com/iberi22/xavier/issues
- **Docs site:** https://xavier.swal.dev (Starlight)
- **API docs:** `docs/API.md`
- **CLI docs:** `docs/CLI.md`
- **Architecture:** `docs/ARCHITECTURE.md`
- **Feature status:** `.gitcore/features.json`

---

_Última actualización: 2026-07-09_
