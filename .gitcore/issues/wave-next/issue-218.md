# [Ola 6] feat-mcp-tools-v2 — Implement MCP Tools v2

> Ola 6 — Core.
> Labels: `ola6`, `wave-next`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push
- [ ] SI el trabajo no se pudo completar: NO abrir PR — comentar el blocker en el issue
- [ ] Verificar que `git show HEAD --name-only` lista LOS MISMOS archivos fuente que el título del PR describe. Si solo hay Cargo.toml/Cargo.lock → el trabajo NO está entregado: NO abrir PR, seguir trabajando.

## Current State (MEDIBLE)
- File: `src/server/mcp/types.rs` (needs StructuredContent support)
- File: `src/server/mcp/tools_memory.rs` (currently `search_memory` returns full text instead of just candidates)
- Feature: `feat-mcp-server` at 100% in `.gitcore/features.json` but needs v2 spec compliance.
- Tests: N existing passing in `src/server/mcp/tests.rs`

## Desired State (DELTA)
- **Section A (types.rs)**: Add MCPContent enum, MCPSearchResult, MCPProvenance, MCPContextResult, MCPHealthResult. Keep MCPToolResult but update it to use MCPContent.
- **Section B (tools_core.rs)**: Update `get_project_context` with params max_records, max_chars, depth. Return structured MCPContent. Update `health_check` to return MCPHealthResult.
- **Section C (tools_memory.rs)**: Rename `search_memory` to `mem_search` (keep alias). `mem_search` returns `MCPSearchResult[]` (candidates only). Add NEW tool `mem_context` taking memory_ids[], project_id, max_chars and fetching full content.
- **Section D (server.rs)**: Route mem_search and mem_context handlers. Support structuredContent.
- **New tests**: In `src/server/mcp/tests.rs` add tools_health_check, error_handling, size_limits.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries. El agente DEBE investigar antes de implementar.**
1. search: "MCP Model Context Protocol structuredContent spec"
2. search: "Rust axum json serialization enum untagged"
3. search: "Rust serde json Value structured output"
4. search: "Xavier memory search architecture Rust"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Research the latest MCP specification for `structuredContent` and tool responses.
2. Read and understand these existing files:
   - `src/server/mcp/types.rs` — note the serialization format
   - `src/server/mcp/tools_memory.rs` — understand how tools interact with the memory layer
3. Make sure not to break existing tool names if clients depend on aliases (keep backward compatibility).
4. Document your findings before writing any code"

## Existing Code Patterns (DEBES seguir estos)
- `src/server/mcp/types.rs` → Serialize/Deserialize serde traits for JSON boundaries.
- `src/server/mcp/tools_core.rs` → Tool response builder pattern.
- `src/server/mcp/tests.rs` → Using `tokio::test` for async testing of MCP methods.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `cargo clippy --package xavier -- -D warnings` — 0 warnings/errors
- [ ] `grep -c "enum MCPContent" src/server/mcp/types.rs` >= 1
- [ ] `cargo test --package xavier --lib server::mcp` 2>&1 | grep "test result: ok" — 1 match
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` — >= 1
- [ ] `git show HEAD --name-only | grep -cE "src/"` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/server/mcp/types.rs` | Types | Add MCPContent and Result types | MED |
| `src/server/mcp/tools_core.rs` | Core tools | Update get_project_context & health | LOW |
| `src/server/mcp/tools_memory.rs` | Memory tools | Add mem_search & mem_context | HIGH |
| `src/server/mcp/server.rs` | MCP router | Route new tools | MED |
| `src/server/mcp/tests.rs` | Tests | Add new tool tests | LOW |
| `src/server/mcp/mod.rs` | Mod file | Update pub use exports | LOW |

## DO NOT touch (Anti-Regression)
- `src/memory/*` — (File island boundary!)
- `.gitcore/features.json` — reconciled at wave end
- NO crear archivos fuera de `src/server/mcp/`

## Anti-Hallucination Guard ⚠️
1. **READ before write**: Leer el archivo COMPLETO antes de modificarlo
2. **Match existing patterns**: Usar el mismo estilo de serializers.
3. **No inventar imports**: Verificar que los crates existen en Cargo.toml
4. **Test patterns**: No usar cargo check/test globales si hay problemas de C, aislar tests en el package.
5. El mensaje del commit debe reflejar archivos REALMENTE modificados.

## Verification
```bash
cargo check --package xavier
cargo test --package xavier --lib server::mcp
```

## Dependencies & Merge Order
- **Depends on:** None
- **Blocked by:** None
- **Parallel with:** #14, #124, #170 (different file islands)
- **Merge order within wave:** 1
- **Expected effort:** Medium 1-4h

## Failure Recovery
| If this happens | Action |
|----------------|--------|
| `cargo clippy` fails | Fix errors, do NOT commit broken code |
| File doesn't exist | Run `find . -name "filename" 2>/dev/null` |
| Test fails on new code | Fix test logic or implementation |
