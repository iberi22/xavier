# [Ola 5v2 · 05] Episodic extractive session summaries (LLM off by default)

> **Re-launch** of #500. Parent #496. Gold-standard Jules issue.

## Web Research Required (Jules must search the web)

1. **Extractive summarization algorithms** — search: `extractive summarization first last sentences keyword scoring 2024`, prefer offline methods.
2. **Conversation compaction for agents** — search: `conversation session summary compaction agent memory 2024 2025`.
3. **Token budgets for episodic slots** — search: `MemGPT context packing episodic summary token budget 200 400`.

PR must state why extractive (not LLM) is default for local-first.

## Exact Technical Context

- **File**: `src/memory/episodic.rs` (~630 lines)
- `SessionSummary` ~32+
- `mark_summarized` ~104–111 is a placeholder setter (no generation)
- Context builder still labels last preview: `src/context/builder.rs` `append_episodic_summary` ~140 (`## Episodic Summary (Last Preview)`)
- Multi-layer episodic assembly may use thread previews in CLI handlers (`rg episodic_summaries src/`)

```rust
// ADD pure function (names flexible):
pub fn summarize_extractive(turns: &[(String, String)], max_chars: usize) -> String {
    // include first user, last assistant, up to N bullet keywords — no network
}
```

> CRITICAL: LLM path only if `XAVIER_EPISODIC_LLM=1` (default **off**). Must work offline. DO NOT touch xavier-core/. NEVER `.patch` files.

## Problem

Episodic layer is not a real summary — agents get raw last_preview, wasting tokens and losing session gist.

## Acceptance Criteria

- [ ] Pure-Rust extractive summarizer, length-capped (~1200 chars)
- [ ] Wired into `append_episodic_summary` OR multi-layer episodic path
- [ ] Unit tests with fixture dialogue
- [ ] Default path performs **zero** network/LLM calls
- [ ] `cargo check --workspace` 0 errors
- [ ] `cargo test -p xavier --lib episodic` passes

## Files to Modify

| File | Change |
|---|---|
| `src/memory/episodic.rs` | extractive + tests |
| `src/context/builder.rs` | consume extractive summary (minimal) |

**DO NOT touch:** tools_memory.rs, code-graph/, panel-ui/

## Verification

```bash
cargo check --workspace
cargo test -p xavier --lib episodic
```

## Dependencies and Merge Order

- **Depends on:** nothing
- **Can run in parallel with:** 03, 04, 06, 07, 09
