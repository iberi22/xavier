# [Ola 5 · 03] Unified estimate_tokens helper

> Ola 5 Cost / #496 P1

## Exact Technical Context
- Multiple estimators: whitespace split, len/4, message.tokens
- Create `src/context/token_estimate.rs` or `src/utils/tokens.rs` with `estimate_tokens(text: &str) -> usize` using len/4 default (document choice)
- Replace call sites in context builder + MCP context assembly only (do not boil ocean)

## Acceptance Criteria
- [ ] Single public `estimate_tokens`
- [ ] Used by memory_context total_chars path OR reported estimated_tokens field
- [ ] Unit tests edge empty/unicode
- [ ] cargo check --workspace
- [ ] DO NOT change BM25/RRF

## Merge order
Parallel with 07+ (different files). Prefer before 04.
