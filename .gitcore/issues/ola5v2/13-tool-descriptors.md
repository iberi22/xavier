# [Ola 5v2 · 13] MCP tool descriptors: progressive disclosure wording + schema defaults

> Complements 01/02 so agents choose the right tools. Gold standard.

## Web Research Required (Jules must search the web)

1. **JSON Schema `default` semantics for tool arguments** — search: `json schema default property ignored by agents 2024`, note that descriptions matter more than defaults for LLMs.
2. **Tool description writing for LLM tool selection** — search: `LLM function calling tool description best practices 2024 2025 progressive disclosure`.
3. **MCP tools concept** — https://modelcontextprotocol.io/docs/concepts/tools

## Exact Technical Context

- `get_xavier_memory_tools()` in `src/server/mcp/tools_memory.rs` ~23–230
- `mem_search` description ~43–44
- `memory_context` description ~213–214 + properties including `ids`, `max_chars`

Target description fragments (English OK):

```text
mem_search: "Returns structured candidates {id,path,score,snippet,kind} WITHOUT full body by default. Set include_content=true only when necessary. Use memory_context with ids to page-in full text."

memory_context: "Page-in full/partial content for specific memory ids (preferred) or a query. Honors max_chars and optional max_chars_per_doc."
```

If issue 02 added `max_chars_per_doc`, document it here in the same PR or rebase.

> CRITICAL: Prefer description/schema-only changes. DO NOT change BM25. NEVER `.patch` files.

## Problem

Vague tool descriptions cause agents to pull full content and skip fat-search.

## Acceptance Criteria

- [ ] mem_search + memory_context descriptions updated
- [ ] inputSchema documents ids / max_chars / max_chars_per_doc (if present)
- [ ] `cargo check --workspace` 0 errors
- [ ] No accidental behavior regressions

## Files to Modify

| File | Change |
|---|---|
| `src/server/mcp/tools_memory.rs` | tool descriptors / schema text |

## Verification

```bash
cargo check --workspace
```

## Dependencies and Merge Order

- **Depends on:** after 02 if schema gains `max_chars_per_doc`
- **Can run in parallel with:** 12
