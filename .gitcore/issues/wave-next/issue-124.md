# [Ola 6] feat-embeddings-export — Export Embeddings Tool

> Ola 6 — Data Portability.
> Labels: `ola6`, `wave-next`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- DB: SQLite + sqlite-vec stores embeddings.
- Missing ability to export raw vectors to JSON/Parquet/CSV.

## Desired State (DELTA)
- **New file**: `src/storage/export.rs` for DB extraction queries.
- **New file**: `src/embedding/export.rs` for mapping extraction to output formats (JSONL/CSV).
- **Update**: `src/cli/commands/` (if CLI command needed) or expose as an internal library function.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "sqlite-vec export vectors Rust"
2. search: "Rust export large datasets to JSONL efficient"
3. search: "Parquet writing Rust arrow parquet crate"
4. search: "Xavier memory storage architecture"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Research efficient ways to stream SQLite data to JSONL in Rust.
2. Review how `src/storage/` currently accesses sqlite-vec.
3. Design a streamable export function to avoid OOM on large vector stores."

## Existing Code Patterns (DEBES seguir estos)
- `src/storage/db.rs` → Connection pooling patterns.

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `cargo clippy --package xavier -- -D warnings` — 0 errors
- [ ] `grep -c "pub fn export_embeddings" src/storage/export.rs` >= 1

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/storage/export.rs` | None | Create | MED |
| `src/storage/mod.rs` | Exists | Export module | LOW |
| `src/embedding/export.rs` | None | Create formatters | LOW |
| `src/embedding/mod.rs` | Exists | Export module | LOW |

## DO NOT touch (Anti-Regression)
- `src/server/*`, `src/context/*` — File Islands boundary.

## Anti-Hallucination Guard ⚠️
1. **READ before write**: Leer `src/storage/db.rs` primero para entender las queries de vec.
2. **Streaming**: No cargar toda la tabla en RAM.

## Verification
```bash
cargo check --package xavier
cargo test --package xavier --lib storage::export
```

## Dependencies & Merge Order
- **Parallel with:** #218, #14, #170
- **Merge order within wave:** 3
- **Expected effort:** Medium 1-4h
