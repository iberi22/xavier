# [Ola 6] feat-context-regen — Context Regeneration Engine

> Ola 6 — Cognitive.
> Labels: `ola6`, `wave-next`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `features.json` lists Context Regeneration as an active goal.
- `src/context/pipeline.rs` needs a scheduled regeneration loop to optimize Retrieval parameters.

## Desired State (DELTA)
- **New file**: `src/retrieval/regeneration.rs` implementing logic to re-rank and regenerate semantic summaries.
- **Update**: `src/context/pipeline.rs` to invoke the regeneration cycle.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "RAG context regeneration patterns AI"
2. search: "Rust background task scheduling tokio"
3. search: "Memory re-consolidation vector DB"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Research how to schedule offline context optimization in a Tokio runtime.
2. Look at `src/context/pipeline.rs` for the main retrieval flow.
3. Design a non-blocking regeneration loop."

## Existing Code Patterns (DEBES seguir estos)
- `src/context/pipeline.rs` → Async token streaming patterns.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `cargo clippy --package xavier -- -D warnings` — 0 errors
- [ ] `grep -c "pub struct ContextRegenerator" src/retrieval/regeneration.rs` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/retrieval/regeneration.rs` | None | Create | HIGH |
| `src/retrieval/mod.rs` | Exists | Export module | LOW |
| `src/context/pipeline.rs` | Exists | Wire regenerator | MED |

## DO NOT touch (Anti-Regression)
- `src/storage/*`, `src/server/*` — File Islands boundary.

## Anti-Hallucination Guard ⚠️
1. **READ before write**: Leer `src/context/pipeline.rs`.
2. **Blocking**: Wrap CPU-heavy regeneration in `tokio::task::spawn_blocking` (Golden rule de Xavier!).

## Verification
```bash
cargo check --package xavier
cargo test --package xavier --lib retrieval::regeneration
```

## Dependencies & Merge Order
- **Parallel with:** #218, #14, #124
- **Merge order within wave:** 4
- **Expected effort:** Large 4h+
