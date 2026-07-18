# [Ola 5 · 05] Episodic extractive session summaries

> Advances #500 / #496 P3

## Exact Technical Context
- `src/memory/episodic.rs` mark_summarized placeholder
- HTTP episodic uses last_preview of threads
- Start with **extractive** summary (first/last N messages + keyword bullets) — LLM optional behind flag OFF by default

## Acceptance Criteria
- [ ] Function `summarize_session_extractive(messages) -> String` ≤ ~400 tokens estimate
- [ ] Persist or attach summary for multi-layer episodic path
- [ ] Test fixture conversation → non-empty summary ≠ raw last_preview only
- [ ] cargo check --workspace
- [ ] DO NOT enable paid LLM by default

## Merge order
Parallel with FTS5/security (different files).
