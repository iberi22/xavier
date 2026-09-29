# Wave pageindex — tree-reasoning retrieval (ADR-036)

Spec: `docs/features/specs/FEATURE-pageindex-tree-retrieval.md` | REQ-080..084 | Ledger: `feat-pageindex-tree-core`, `feat-pageindex-pdf`, `feat-pageindex-hybrid` (all `stable`).

**Status: CLOSED 2026-09-29.** All 14 issues shipped; ADR-036 accepted.

Shipped beyond the plan:
- Issue 01b: boundary pages may be shared between sibling nodes (`test_tree_validate_allows_shared_boundary_page`).
- pdfium 6996 binding (`pdfium_6996` feature of pdfium-render, bound at runtime).
- Shared state between MCP and HTTP (`src/pageindex_glue/state.rs`).
- `pageindex_search` in-document search tool (MCP and HTTP).
- PDF quality fixes: glyph-level line rebuild with overprint dedupe, running headers/sentences/page numbers dropped as headings, numbered sections lifted, outline size cap, fixed/split windows titled by first meaningful line.
- PDFs with an empty user password open instead of failing as encrypted.
- End-to-end evaluation on real PDFs and its reproducible harness (`scripts/pageindex-e2e/`).

| # | Title | Phase | Depends on | Parallel group |
|---|-------|-------|-----------|----------------|
| 01 | Scaffold crate + tree data model (+ all module stubs, all deps) | F1 | - | G0 |
| 02 | Markdown tree builder | F1 | 01 | G1 |
| 03 | Plain-text and legal tree builders | F1 | 01 | G1 |
| 04 | SQLite tree store | F1 | 01 | G1 |
| 05 | Facade + query operations | F1 | 02,03,04 | G2 |
| 06 | Xavier glue: features, settings, tool logic, .env.example | F1 | 05 | G3 |
| 07 | HTTP routes /v1/pageindex/* | F1 | 06 | G4 |
| 08 | MCP registration tools_pageindex.rs (rebase on main after V6) | F1 | 06 | G5 (last) |
| 09 | PDF outline builder (lopdf) | F2 | 01 | G6 |
| 10 | PDF layout heading detector (pdfium) | F2 | 01 | G6 |
| 11 | Tree optimize + LLM summaries + adapter | F2 | 05,06 | G6 |
| 12 | PDF cascade pipeline + ingest wiring | F2 | 09,10,11 | G7 |
| 13 | Hybrid retrieval arm + gating hook | F3 | 06,11 | G8 |
| 14 | Eval harness + comparison | F3 | 05 (13 for hybrid column) | G8 |
| 15 | Follow-up: node-summary vector leg (planned, not in this wave) | F3 | 13 | - |

Parallelism (disjoint file islands): after 01 -> {02,03,04} together; 09 and 10 can start right after 01 (they only fill their own stub files) and run alongside F1 issues 02-08; 11 needs 06; {13,14} together after 11. Only issues 01 and 06 touch root `Cargo.toml`; only 08 touches `src/server/mcp/{mod,server}.rs` (other wave V6 owns them: rebase on main after V6 merges); nobody touches `tools_core.rs` or `src/context/**`.

Known: main CI has an intermittent pre-existing failure in "Rust Integration (Observability Contract)" (#2701), not caused by this wave. Jules is unavailable for xavier at the time of writing; use CLI agents.
