# [Ola 5 · 13] Tests: progressive disclosure integration suite

> #497 DoD tests

## Exact Technical Context
- src/server/mcp/tests.rs
- Assert mem_search structured / no full content
- Assert memory_context(ids=[...]) returns only those ids

## Acceptance Criteria
- [ ] ≥2 tests green
- [ ] cargo test -p xavier --lib progressive OR mcp tests filter
- [ ] Only test files + minimal helpers

## Merge order
After 01+02.
