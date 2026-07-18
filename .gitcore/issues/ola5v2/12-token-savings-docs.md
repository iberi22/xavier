# [Ola 5v2 · 12] Docs: honest TOKEN_SAVINGS_ANALYSIS + progressive disclosure playbook

> Parent #496. Docs-only gold-standard issue.

## Web Research Required (Jules must search the web)

1. **Honest measurement of agent token savings** — search: `measure LLM token reduction progressive disclosure methodology 2024 2025`, reject vanity 99% claims without method.
2. **Documenting MCP tools for agents** — search: `documenting MCP tools catalog progressive disclosure best practices 2025`.
3. Skim https://modelcontextprotocol.io/docs/concepts/tools for terminology consistency.

## Exact Technical Context

- File may exist: `docs/TOKEN_SAVINGS_ANALYSIS.md` — rewrite overclaims
- **Actual main contract** (verify in code before documenting):
  - `mem_search`: structured `candidates` with id/path/score/snippet/kind; `include_content` default false (`tools_memory.rs` ~237–297)
  - `memory_context`: `query` or `ids`, `max_chars`, per-doc fair-share (~714–862)
- Link open Ola 5v2 issues for remaining gaps (WorkingMemory, episodic, etc.)

> CRITICAL: Documentation only. No Rust unless fixing a broken example that already fails. NEVER `.patch` files.

## Problem

Historical docs overclaim savings; agents need an accurate progressive disclosure playbook matching main.

## Acceptance Criteria

- [ ] Document real JSON shapes from main
- [ ] Remove/reword unsubstantiated 99% claims; add measurement recipe
- [ ] List remaining gaps honestly
- [ ] Valid markdown

## Files to Modify

| File | Change |
|---|---|
| `docs/TOKEN_SAVINGS_ANALYSIS.md` | rewrite |
| optional related MCP docs | cross-link |

## Verification

```bash
# docs only
test -f docs/TOKEN_SAVINGS_ANALYSIS.md
```

## Dependencies and Merge Order

- **Depends on:** soft after 01/02
- **Before:** 14
