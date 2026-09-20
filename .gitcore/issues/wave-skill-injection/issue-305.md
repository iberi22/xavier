# [Ola skill-injection] feat-skill-mcp-tool — MCP Dispatch Tools

> Ola skill-injection — Local-first intelligent skill injection, 100% offline (no Jev API).
> Labels: `wave-skill-injection`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `src/server/mcp/tools_context.rs:22` — `get_xavier_context_tools()` returns save/restore/search (+ issue-packaging per module docs). No skill tools.
- `src/server/mcp/tools_context.rs:103` — `handle_context_tool` dispatches by tool name.
- REST precedent: `POST /api/skill/dispatch {task, model_hint?, max_tokens?=4000, project?}` and `GET /api/skill/list`, `GET /skills` (`src/api/skills.rs`, routes `src/cli/server.rs:1006-1013`).
- `MemoryRefResponse {id, path, summary, keywords}` shape already exists for reuse.

## Desired State (DELTA)
- **Update**: `src/server/mcp/tools_context.rs` — register `xavier_dispatch_skill` (`{task*, max_tokens?, project?}`, ≤60-char plain descriptions per Hermes `SKILL_PROMPT_DESC_LIMIT` lesson) and `xavier_skill_list` (no required params), reusing the exact dispatcher construction from `api/skills.rs::dispatch_skill`.
- **Update**: `handle_context_tool` arms for both names; dispatch returns `{skill_name, skill_description, confidence, context_pack, estimated_savings_pct}` JSON; fail-open on error (empty skill + reason, never a throw).
- **Keep**: existing tool names/schemas byte-identical.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "MCP server tool inputSchema best practices descriptions"
2. search: "Model Context Protocol tools/list handler routing Rust"
3. search: "fail-open tool design empty result vs error MCP"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Read `get_xavier_context_tools` + `handle_context_tool` fully plus one full tool arm as template.
2. Read `api/skills.rs::dispatch_skill` — mirror its dispatcher wiring, do not duplicate ranking logic.
3. Check how MCP transport tests are written (`src/server/mcp/tests.rs`) and extend that file."

## Existing Code Patterns (DEBES seguir estos)
- `tools_context.rs` → `MCPTool { name, description, input_schema }` + handler arm pattern.
- `api/skills.rs` → response shapes (`DispatchResponse`, `SkillListEntry`).

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] MCP `tools/list` output contains `xavier_dispatch_skill` and `xavier_skill_list`
- [ ] Round-trip `xavier_dispatch_skill {task:"...", max_tokens:2000}` returns skill + pack within budget
- [ ] `xavier_skill_list` count matches REST `GET /skills` count (same registry)
- [ ] `cargo test -p xavier --lib server::mcp` — green incl. 2 new tests; clippy `-D warnings` clean; fmt clean

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/server/mcp/tools_context.rs` | Exists | 2 tools + 2 handler arms | LOW |
| `src/server/mcp/tests.rs` | Exists | Round-trip tests | LOW |

## DO NOT touch (Anti-Regression)
- Existing MCP tool names/schemas.
- Dispatcher/registry internals (owned by #301/#302).
- MCP transport/auth layers.

## Anti-Hallucination Guard ⚠️
1. **Mirror, don't fork**: ranking logic lives in exactly one place (dispatcher).
2. **Description budget**: tool descriptions short and functional (lesson: >60 chars breaks some approvers).
3. **Fail-open**: errors serialize to `{ok:false,...}` or empty-skill verdicts, never panics across the MCP boundary.

## Verification
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p xavier --lib server::mcp
# live tools/list grep (command in PR)
```

## Dependencies & Merge Order
- **Depends on:** #301 (non-empty registry), #302 (confidence)
- **Parallel with:** #304
- **Merge order within wave:** 5
- **Expected effort:** Medium 2h
