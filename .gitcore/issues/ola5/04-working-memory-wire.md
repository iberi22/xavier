# [Ola 5 · 04] Wire WorkingMemory; stop all_documents multi-layer hot path

> Ola 5 Cost / #496 P2

## Exact Technical Context
- Search `all_documents` in `src/server/http/api.rs` and multi-layer retrieve
- WorkingMemory types may exist under src/memory or workspace
- Goal: working layer = recent/hot ids only, not full corpus

## Acceptance Criteria
- [ ] Multi-layer working candidates not full DB dump
- [ ] Tests or bench note
- [ ] cargo check --workspace
- [ ] Minimal files; list every path in PR description

## Merge order
After 01 preferred.
