# [Ola skill-injection] feat-skill-semantic-rank — Semantic Ranking via Local Embeddings

> Ola skill-injection — Local-first intelligent skill injection, 100% offline (no Jev API).
> Labels: `wave-skill-injection`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `src/context/skill_registry.rs:200-235` — `score_skill_match` is pure keyword counting (name/desc/domain substring hits, capped at 1.0). No semantics: "review a pull request" will not match a skill described as "verify GitHub PRs with evidence" unless tokens overlap.
- Embedding infra already exists: `src/ports/outbound/embedding_port.rs` (port), `src/memory/embedder.rs`, `sqlite-vec` store per `feat-unified-storage`.
- No eval harness exists for skill ranking quality.

## Desired State (DELTA)
- **Update**: `src/context/skill_registry.rs` — at `index_skill_file` time, embed `name + first 200 chars of description` through `EmbeddingPort`; persist vector alongside `IndexedSkill` (reuse sqlite-vec patterns from memory store, new table/namespace, not a second DB).
- **Update**: `search()` ranks by cosine similarity top-K; keyword `score_skill_match` becomes tiebreak; returned confidence is the cosine score clamped 0–1.
- **New**: offline eval harness (`tests/` or benches, TBD by implementer following repo layout) with ≥20 labeled queries (`query → expected skill`), reporting Recall@3 ≥ 0.8 and MRR. Eval vectors come from a mocked port — zero network in tests.
- **Keep**: `search()` signature compatible for existing callers (`skill_dispatcher`, `list_skills`).

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "sqlite-vec cosine similarity top-K Rust rusqlite"
2. search: "hybrid keyword + vector rerank tiebreak RAG small catalog"
3. search: "Recall@K MRR offline eval harness information retrieval"
4. search: "hexagonal architecture embedding port mock Rust tests"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Read `src/ports/outbound/embedding_port.rs` + `src/memory/embedder.rs` to see how memories get vectors today — reuse, do not reinvent.
2. Read `score_skill_match` + `search` fully; keep the keyword path as fallback when no vectors exist (fail-open).
3. Check `src/context/skill_dispatcher.rs` for how confidence is consumed downstream."

## Existing Code Patterns (DEBES seguir estos)
- `src/memory/*` → embedding + sqlite-vec storage patterns.
- `src/context/skill_registry.rs` → registry + test module patterns (`#[cfg(test)]` at file bottom).
- Golden rule (Tokio + Rayon): CPU-heavy cosine loops over 349 skills go in `spawn_blocking` if called from async (AGENTS.md §8).

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] Eval harness reports Recall@3 ≥ 0.8 and MRR on the labeled set (paste numbers in PR)
- [ ] `cargo test -p xavier --lib context::skill_registry` — green, incl. `test_confidence_calibration_range` (all scores in [0,1]) and `test_ranking_offline_no_network` (no socket use — run with network namespace blocked if available)
- [ ] Keyword-only fallback still works when vector store is empty (test with fresh temp registry)
- [ ] `cargo clippy --all-targets -- -D warnings` — 0 warnings; `cargo fmt --check` — clean

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/context/skill_registry.rs` | Exists | Embed at index + cosine rank | MED |
| `src/memory/embedder.rs` or port impl | Exists | Only if a new port method is needed; prefer reuse | LOW |
| Eval harness file | None | Create (location per repo test layout) | LOW |

## DO NOT touch (Anti-Regression)
- Embedding model selection / Ollama wiring (`nomic-embed-text` stays).
- `src/server/*` route contracts.
- Any secret, token, or `.env` handling.

## Anti-Hallucination Guard ⚠️
1. **READ before write**: embedding port + 2 call sites of `registry.search`.
2. **No cloud**: grep the diff for `reqwest|http.*api|api_key` in new ranking code — must be absent (offline mandate).
3. **Calibrated != invented**: confidence must derive from measured cosine, never from constants.

## Verification
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test -p xavier --lib context::skill_registry
# eval harness run (exact command in PR) → Recall@3, MRR printed
```

## Dependencies & Merge Order
- **Depends on:** #301 (index-time embedding hooks into the same file)
- **Blocks:** #304, #305 (they consume confidence)
- **Merge order within wave:** 2
- **Expected effort:** Large 4h+
