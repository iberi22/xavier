# [Ola 5 · 02] memory_context: per-doc max_chars + total budget honesty

> Ola 5 Cost / #497 / #496

## Web Research
1. Context window budgeting for RAG tools 2024

## Exact Technical Context
- `memory_context` at tools_memory.rs ~708+
- Already accepts `ids` and global `max_chars` (default 4000)
- Gap: may still load full docs then truncate once; need per-doc cap (e.g. max_chars/n or explicit `max_chars_per_doc`)

## Acceptance Criteria
- [ ] Param `max_chars_per_doc` optional (default min(800, max_chars))
- [ ] Each source content truncated independently
- [ ] Response reports total_chars + truncated flags honestly
- [ ] Tests with multi-id page-in
- [ ] cargo check --workspace
- [ ] Only tools_memory.rs + tests

## Merge order
After or with 01 (same file — **serialize: 01 then 02**).
