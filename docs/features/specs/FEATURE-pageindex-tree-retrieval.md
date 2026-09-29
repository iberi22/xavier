# FEATURE: PageIndex-style Tree-Reasoning Retrieval

**Base:** `origin/main bcfc2e06` | **ADR:** ADR-036 | **REQ:** REQ-080..084
**Ledger:** `feat-pageindex-tree-core` (F1), `feat-pageindex-pdf` (F2), `feat-pageindex-hybrid` (F3)
**Status:** `stable` (ledger promoted; ADR-036 accepted) | **Wave:** `.gitcore/waves/wave-pageindex/`

---

## 1. Overview & scope

Port the idea of VectifyAI/PageIndex (MIT) to Rust as the independent workspace crate
`crates/xavier-pageindex`: index each document as a hierarchical ToC tree and let the
calling agent navigate it with five read tools. Xavier needs no LLM at query time.

In scope: tree model, builders (markdown, plain text, legal, PDF), storage, 6 MCP
tools, HTTP routes, optional node summaries, tree optimize, hybrid retrieval arm, eval.
Out of scope: OCR of scanned PDFs, image/table extraction, cloud PageIndex API, any
change to `src/context/skill_controller/*`, Xavier core SQLite migrations.

Not in scope by ownership constraint: `src/server/mcp/{mod,server,tools_core}.rs` are
edited by another wave (V6 secrets). Only issue 08 touches `mod.rs`/`server.rs`, with
minimal lines, and MUST be rebased on main after V6 merges.

### Verified reuse (read from the repo)

| Asset | Finding | Use |
|-------|---------|-----|
| `src/documents/legal_chunker.rs` | Regex table (`CHAPTER/ARTICLE/CLAUSE/PARAGRAPH/PREAMBLE_REGEX`, lines 67-99) and `full_path` breadcrumbs; produces flat `LegalChunk`s | Regexes ported into `builders/legal.rs`; parity test vs chunker fixture |
| `src/documents/pdf.rs` | `extract_pdf_document(path, bytes)`, hand-rolled `BT/ET`, flate2; no fonts/positions | NOT enough for headings. Keep as-is; crate gets own PDF path |
| `src/memory/hierarchy.rs` | `MemoryTree::build_ls` builds a directory listing of memory paths | Not a document tree; only a naming precedent. Not reused |
| `src/rag/llm_adapter.rs` | `LlmAdapterTrait::complete(&str)`, `LlmConfig::from_env()` (`DOCBOT_LLM_*`), Ollama/OpenAI/RetrievalOnly | Summarizer adapter in `src/pageindex_glue/summarizer.rs`; `RetrievalOnly` => no summaries |
| `src/retrieval/gating.rs` | `AdaptiveGating`, RRF via `crate::search::rrf::reciprocal_rank_fusion`, `rrf_k` | Arm results fused as an extra ranked list |
| `src/retrieval/eval.rs` | `EvalDataset`, `RetrievalMetrics`, `is_hit` (text based) | Pattern for the page-range eval; text `is_hit` not reused |
| `src/server/mcp/*` | Tools defined in `get_xavier_*_tools()`, dispatched in `handle_tool_call` (`server.rs:105-118`) by name/prefix; role gate by tool name in `server.rs` | New module `tools_pageindex.rs`, prefix `pageindex_` |
| `src/server/docbot_routes.rs` + `src/cli/server.rs:~818` | Router built per-feature and `.merge`d/nested in the main app | Same pattern for `pageindex_routes.rs` |

## 2. Data model (Rust sketch, crate `xavier-pageindex`)

```rust
/// Address unit of a document. PDFs use pages; text/markdown use virtual pages
/// (fixed line windows, `lines_per_page`, default 60) so every tool works in "pages".
pub enum PageUnit { Page, VirtualPage }

pub struct Document {
    pub doc_id: String,          // uuid v4
    pub workspace: String,       // Xavier namespace, isolation key
    pub name: String,            // unique per workspace (case-sensitive)
    pub source_kind: SourceKind, // Markdown | PlainText | Legal | Pdf
    pub content_hash: String,    // sha256 of raw bytes; idempotent re-ingest
    pub page_count: u32,
    pub page_unit: PageUnit,
    pub status: DocStatus,       // Processing | Completed | Failed(String)
    pub builder: String,         // "markdown" | "pdf-bookmarks" | "pdf-layout" | "pdf-llm" | ...
    pub created_at: i64,
}

pub struct TreeNode {
    pub node_id: String,         // zero-padded preorder "0000", "0001", ...
    pub title: String,
    pub start_page: u32,         // 1-based inclusive
    pub end_page: u32,           // 1-based inclusive
    pub summary: Option<String>, // None when no LLM
    pub token_estimate: u32,
    pub children: Vec<TreeNode>,
}

pub struct DocumentTree { pub doc_id: String, pub roots: Vec<TreeNode> }

pub struct Page { pub doc_id: String, pub page_no: u32, pub text: String }

pub trait Summarizer: Send + Sync {
    fn summarize(&self, title: &str, text: &str, max_words: usize)
        -> Pin<Box<dyn Future<Output = Result<String, PageIndexError>> + Send + '_>>;
}

pub enum PageIndexError {
    NotFound(String), InvalidRange(String), FeatureDisabled(&'static str),
    PdfiumUnavailable(String), Encrypted, Store(String), Build(String),
}
```

Invariants (`DocumentTree::validate`): siblings ordered and non-overlapping (adjacent siblings may share exactly one boundary page: `prev.end <= next.start`; deeper overlap is rejected); child
range inside parent; `1 <= start <= end <= page_count`; `node_id` unique, preorder.

### Storage decision: own SQLite file, WAL, owned by the crate

`$XAVIER_PAGEINDEX_DB` (default `<XAVIER data dir>/pageindex.sqlite3`). Rationale:
keeps the crate standalone (no `xavier` dep), leaves Xavier migrations
(`src/storage/migrations.rs`) untouched, and rusqlite (`bundled`) is already in the
workspace. Node summaries are additionally embedded into the Xavier store by the
hybrid arm (F3) — that is the only place the two stores meet.
Tables: `pi_documents`, `pi_nodes(doc_id,node_id,parent_id,ord,title,start_page,end_page,summary,token_estimate)`,
`pi_pages(doc_id,page_no,text)`; `FOREIGN KEY ... ON DELETE CASCADE`,
`UNIQUE(workspace,name)`. Runtime `*.sqlite3` is never committed (AGENTS.md sec. 10).

## 3. Crate layout

```
crates/xavier-pageindex/
  Cargo.toml
  src/lib.rs            // re-exports, PageIndex facade
  src/error.rs          // PageIndexError (thiserror)
  src/model.rs          // Document, TreeNode, DocumentTree, validate()
  src/builders/{mod,markdown,plain,legal}.rs
  src/pdf/{mod,outline,layout,cascade}.rs     // feature-gated
  src/optimize.rs       // merge tiny / split huge
  src/summarize.rs      // Summarizer trait + bottom-up pass + content-hash cache
  src/store/{mod,sqlite,schema}.rs
  src/query.rs          // structure projection, page-range parser, similar-name hints
  src/eval/{mod,metrics}.rs                   // page-range metrics (F3)
  tests/                // integration tests + fixtures/
```

Xavier-side glue (new, isolated files): `src/pageindex_glue/{mod,state,tools,summarizer}.rs`,
`src/server/pageindex_routes.rs`, `src/server/mcp/tools_pageindex.rs`,
`src/retrieval/pageindex_arm.rs`, `tests/pageindex_eval.rs`.

## 4. Cargo features and PDF/pdfium strategy

Crate `xavier-pageindex`:

| Feature | Deps | Default | Purpose |
|---------|------|---------|---------|
| `sqlite` | `rusqlite` (bundled) | yes | store |
| `markdown` | `pulldown-cmark` | yes | markdown builder |
| `pdf-outline` | `lopdf` (pure Rust) | no | bookmarks + per-page text |
| `pdf-layout` | `pdfium-render` (dynamic binding only; do NOT enable its `static` feature) | no | font-size/weight heading detection |

Xavier crate (root `Cargo.toml`): `pageindex = ["dep:xavier-pageindex"]` (added to
`default` and `ci-safe`, no native deps), `pageindex-pdf = ["pageindex", "xavier-pageindex/pdf-outline"]`,
`pageindex-pdfium = ["pageindex-pdf", "xavier-pageindex/pdf-layout"]` (never in `default`/`ci-safe`).

pdfium provisioning (checked: `nixpkgs#pdfium-binaries` exists, version 7087):

- `pdfium-render` loads `libpdfium` at RUNTIME (`Pdfium::bind_to_library`/
  `bind_to_system_library`), so `cargo build` needs no native lib in any config.
- Lookup order: `XAVIER_PAGEINDEX_PDFIUM_LIB` (dir or file) -> `PDFIUM_DYNAMIC_LIB_PATH`
  -> `./libpdfium.so` next to the binary -> system library. Missing => `PdfiumUnavailable`,
  and the cascade falls through to the next strategy.
- NixOS: add `pdfium-binaries` to `shell.nix` buildInputs behind a comment and export
  `PDFIUM_DYNAMIC_LIB_PATH=${pkgs.pdfium-binaries}/lib`; the systemd user unit sets the
  same env. Not part of this F0 phase (no shell.nix edit).
- CI: default jobs unchanged. One optional job (`pageindex-pdfium`, not required)
  downloads a pinned `bblanchon/pdfium-binaries` release with checksum and runs
  `cargo test -p xavier-pageindex --features pdf-layout -- --include-ignored`.
  pdfium tests are `#[ignore]` unless `XAVIER_PAGEINDEX_PDFIUM_LIB` resolves.

## 5. Public API (crate)

```rust
impl PageIndex<S: Store> {
    pub fn new(store: S, cfg: PageIndexConfig) -> Self;
    pub async fn ingest(&self, ws: &str, name: &str, src: Source<'_>, opts: IngestOptions)
        -> Result<Document, PageIndexError>;         // idempotent on content_hash
    pub fn browse(&self, ws: &str, q: BrowseQuery) -> Result<BrowsePage, PageIndexError>;
    pub fn get_document(&self, ws: &str, name: &str) -> Result<DocumentInfo, PageIndexError>;
    pub fn get_structure(&self, ws: &str, name: &str, opts: StructureOpts)
        -> Result<StructureView, PageIndexError>;    // text stripped, char-budgeted
    pub fn get_pages(&self, ws: &str, name: &str, pages: &str /* "5-7,12" */, cap: usize)
        -> Result<Vec<Page>, PageIndexError>;
    pub fn delete(&self, ws: &str, name: &str) -> Result<(), PageIndexError>;
}
pub enum Source<'a> { Markdown(&'a str), PlainText(&'a str), Legal(&'a str), Pdf(&'a [u8]) }
pub struct IngestOptions { pub summarize: bool, pub optimize: bool, pub lines_per_page: u32 }
```

## 6. MCP tools (module `src/server/mcp/tools_pageindex.rs`)

All names prefixed `pageindex_`; descriptions <= 200 chars. Outputs are one JSON text
content (`{"ok":true,...}` or `{"ok":false,"error":"...","hint":"..."}`), matching the
skill-tool convention; no tool throws.

| Tool | Params (* required) | Output |
|------|--------------------|--------|
| `pageindex_browse_documents` | `query?`, `limit?` (default 20, max 100), `offset?` | `{ok, total, documents:[{name, page_count, status, builder, node_count}]}`; `query` ranks by title/summary BM25-lite |
| `pageindex_get_document` | `doc_name*` | `{ok, name, status, page_count, page_unit, builder, has_summaries, structure_first: bool}` (`structure_first` true when `page_count > 20`, as PageIndex) |
| `pageindex_get_document_structure` | `doc_name*`, `max_depth?`, `node_id?` (subtree) | `{ok, name, structure:[{node_id,title,start_page,end_page,summary?,nodes:[...]}], truncated: bool}`; never includes page text; truncated by `XAVIER_PAGEINDEX_MAX_RESPONSE_CHARS`. Navigation hints per node: `hidden_children` + `descendant_titles_sample` (first 4 hidden descendant titles) when children are cut by depth/budget, and `too_large_for_one_call: true` when the span exceeds 20 pages (next_step: drill down with `node_id` or use `pageindex_search`). Printed page labels are not exposed: the model stores no labels |
| `pageindex_get_page_content` | `doc_name*`, `pages*` (`"5-7,12"`, max 20 pages/call) | `{ok, name, pages:[{page, text}], truncated}`; out-of-range => error with valid range |
| `pageindex_search` | `doc_name*`, `query*`, `limit?` (default 10, max 30) | `{ok, doc_name, query, hits:[{page_no, node_id, breadcrumb:[titles], score, snippet}], total_matches, truncated, next_steps}`; BM25 over pages plus node title/summary (heading pages get a boost); snippets are ~220 chars around the first match, never full pages; read-only (no write gating); unknown doc => error with similar names |
| `pageindex_index_document` (write) | `doc_name*`, exactly one of `path` / `content*`, `format?` (`markdown\|text\|legal\|pdf`, auto by extension), `summarize?` (default false), `optimize?` (default true) | `{ok, name, page_count, node_count, builder, unchanged: bool}`; requires a role that may write memory; `path` must sit under `XAVIER_PAGEINDEX_INGEST_ROOTS` |

Error hints for unknown `doc_name`: up to 3 closest names (difflib-style), as PageIndex.
Role gating for `pageindex_index_document` is enforced INSIDE the handler (uses the
`Role` passed by the dispatcher) so `server.rs` needs no per-tool arm.

## 7. HTTP routes (`src/server/pageindex_routes.rs`, nested under `/v1/pageindex`, same auth middleware as DocBot)

| Method | Path | Maps to |
|--------|------|---------|
| GET | `/v1/pageindex/documents?query=&limit=&offset=` | browse |
| GET | `/v1/pageindex/documents/{name}` | get_document |
| GET | `/v1/pageindex/documents/{name}/structure?max_depth=&node_id=` | structure |
| GET | `/v1/pageindex/documents/{name}/pages?pages=5-7,12` | page content |
| GET / POST | `/v1/pageindex/documents/{name}/search?query=&limit=` (POST body `{query, limit?}`) | in-document search |
| POST | `/v1/pageindex/documents` | ingest (`{name, path\|content, format?, summarize?, optimize?}`) |
| DELETE | `/v1/pageindex/documents/{name}` | delete |

## 8. Environment variables (add each to `.env.example` in issue 06)

| Variable | Default | Purpose |
|----------|---------|---------|
| `XAVIER_PAGEINDEX_DB` | `<data dir>/pageindex.sqlite3` | tree store path |
| `XAVIER_PAGEINDEX_INGEST_ROOTS` | empty (=> `path` ingest disabled, `content` only) | comma-separated allowed directories for `path` ingest |
| `XAVIER_PAGEINDEX_MAX_RESPONSE_CHARS` | `95000` | per-tool response cap |
| `XAVIER_PAGEINDEX_LINES_PER_PAGE` | `60` | virtual page size for text/markdown |
| `XAVIER_PAGEINDEX_SUMMARIZE` | `false` | summarize on ingest by default |
| `XAVIER_PAGEINDEX_SUMMARY_MAX_WORDS` | `60` | summary length |
| `XAVIER_PAGEINDEX_MIN_NODE_TOKENS` | `200` | merge threshold (optimize) |
| `XAVIER_PAGEINDEX_MAX_NODE_TOKENS` | `6000` | split threshold (optimize) |
| `XAVIER_PAGEINDEX_PDFIUM_LIB` | unset | pdfium dir/file (F2) |
| `XAVIER_PAGEINDEX_ARM_ENABLED` | `false` | hybrid arm on/off (F3) |
| `XAVIER_PAGEINDEX_ARM_WEIGHT` | `1.0` | RRF weight of the arm (F3) |

Reused, not new: `DOCBOT_LLM_BACKEND`, `DOCBOT_LLM_MODEL`, `DOCBOT_OLLAMA_URL`,
`DOCBOT_OPENAI_URL`, `DOCBOT_OPENAI_API_KEY` (already documented). The crate itself
reads NO env; all env is read once in `src/pageindex_glue/mod.rs` (`PageIndexSettings::from_env`).

## 9. Test plan (concrete names; crate tests via `cargo test -p xavier-pageindex`, Xavier tests via `cargo test -p xavier --lib --features ci-safe <filter>`)

**F1 (`feat-pageindex-tree-core`)**
- model: `test_tree_node_serde_roundtrip_stable_json`, `test_node_ids_are_zero_padded_preorder`, `test_tree_validate_rejects_overlapping_siblings`, `test_tree_validate_rejects_child_outside_parent_range`
- markdown: `test_md_headings_build_nested_tree`, `test_md_ignores_hashes_inside_code_fences`, `test_md_skipped_heading_levels_attach_to_nearest_ancestor`, `test_md_node_ranges_cover_all_pages`, `test_md_preamble_becomes_root_node`
- plain/legal: `test_plain_numbered_headings_detected`, `test_plain_no_headings_falls_back_to_fixed_windows`, `test_legal_articles_nest_under_chapters`, `test_legal_spanish_and_english_headings`, `test_legal_parity_with_legal_chunker_fixture` (Xavier side)
- store: `test_store_roundtrip_document_tree`, `test_store_get_pages_by_range`, `test_store_reingest_same_hash_is_idempotent`, `test_store_delete_cascades_nodes_and_pages`, `test_store_wal_and_foreign_keys_enabled`, `test_store_workspace_isolation`
- facade/query: `test_browse_lists_docs_with_status_and_page_count`, `test_get_document_structure_strips_text`, `test_structure_truncates_to_char_budget_with_marker`, `test_get_page_content_parses_ranges`, `test_get_page_content_rejects_out_of_range`, `test_get_page_content_respects_response_char_cap`, `test_unknown_doc_returns_similar_names`
- glue: `test_tool_specs_names_and_required_params`, `test_tool_errors_are_envelopes_not_panics`, `test_ingest_tool_denied_for_readonly_role`, `test_ingest_path_outside_roots_rejected`, `test_settings_from_env_defaults`
- HTTP: `test_pageindex_routes_browse_ok`, `test_pageindex_routes_structure_404_unknown_doc`, `test_pageindex_routes_require_auth`
- MCP: `test_mcp_pageindex_tools_announced`, `test_mcp_pageindex_roundtrip_ingest_structure_pages`, `test_mcp_existing_tool_names_unchanged`

**F2 (`feat-pageindex-pdf`)**
- outline: `test_pdf_outline_bookmarks_to_tree`, `test_pdf_outline_resolves_named_destinations`, `test_pdf_without_outline_returns_none`, `test_pdf_page_text_extracted_per_page`, `test_pdf_encrypted_returns_typed_error`
- layout: `test_layout_font_size_clusters_map_to_levels`, `test_layout_headers_footers_filtered`, `test_layout_bold_short_line_is_heading_candidate`, `test_layout_missing_pdfium_returns_typed_error_not_panic`, `test_layout_pdfium_path_resolution_env_order`
- optimize/summaries: `test_optimize_merges_tiny_leaf_into_prev_sibling`, `test_optimize_splits_node_over_token_limit_on_page_boundaries`, `test_optimize_preserves_page_coverage`, `test_optimize_is_idempotent`, `test_summarize_bottom_up_order`, `test_summarize_cache_hit_skips_llm`, `test_summarize_failure_leaves_summary_none_and_tree_valid`, `test_summarizer_adapter_uses_llm_config`, `test_tree_builds_with_no_llm_configured`
- cascade: `test_pdf_cascade_prefers_bookmarks`, `test_pdf_cascade_falls_back_to_layout`, `test_pdf_cascade_falls_back_to_llm_toc_when_no_layout`, `test_pdf_cascade_final_fallback_fixed_windows`, `test_pdf_ingest_default_features_returns_feature_disabled_error`

**F3 (`feat-pageindex-hybrid`)**
- arm: `test_arm_embeds_node_summaries_with_doc_and_node_tags`, `test_arm_reingest_replaces_stale_embeddings`, `test_arm_results_carry_doc_node_page_range`, `test_arm_workspace_scoped`, `test_gating_arm_disabled_by_default_no_behavior_change`, `test_gating_rrf_fuses_arm_with_layers`, `test_gating_arm_failure_is_fail_open`
- eval: `test_eval_page_range_hit_metric`, `test_eval_hit_at_k_and_mrr`, `test_eval_loads_benchmark_jsonl`, `test_eval_fixture_set_has_gold_ranges`, `test_eval_tree_navigator_lexical_baseline_deterministic`, `test_eval_report_compares_docbot_tree_hybrid`

Eval metric (LLM-free): a query is a HIT@k if any returned span overlaps the gold page
range `[gs,ge]` of the same document within top-k; MRR over first hit. The tree arm's
LLM-free baseline is a deterministic lexical navigator (BM25 over titles+summaries,
descend to best child), a lower bound for what an agent achieves. Datasets: in-repo
fixtures `tests/fixtures/pageindex/` (>= 3 docs, >= 30 queries, gold ranges) and
optional `PAGEINDEX_OSS_BENCHMARK_DIR` JSONL (never downloaded in CI).

## 10. Acceptance criteria

- F1: `cargo test -p xavier-pageindex` green; `cargo tree -p xavier-pageindex` shows no `xavier` package; MCP `tools/list` shows 6 new tools and prior tool names byte-identical; ingest -> structure -> pages round-trip live on :8006 after restart; `cargo clippy --all-targets -- -D warnings` clean; default/`ci-safe` build has no new native dep.
- F2: default-features `cargo check -p xavier-pageindex` needs no pdfium; PDF with bookmarks produces a valid tree; PDF without bookmarks and without pdfium degrades to LLM-ToC/fixed windows with a `builder` label saying so; optimize preserves page coverage and is idempotent; a tree builds with zero LLM configuration.
- F3: arm off by default with byte-identical existing gating tests; on, it returns nodes with page ranges; eval report prints hit@1/hit@3/MRR for DocBot, tree, hybrid and the numbers are recorded in this spec; decision review per ADR-036 invalidation criteria.
- Ledger: promotion only by green `scripts/verify-pipeline.sh`, never by hand.

### Eval results (PAGEINDEX.14, measured)

Command: `cargo test -p xavier --features ci-safe --test pageindex_eval -- --nocapture`.
Data: 3 committed fixtures in `tests/fixtures/pageindex/` (`manual.md` 13 pages, `contract.txt` 11 pages,
`handbook.txt` 11 pages; 8 lines per virtual page) and 36 questions in `gold.jsonl`, each with a gold page
range and an `evidence` phrase that a test checks lies inside that range. No LLM, no network.
A hit means a returned span overlaps the gold range in the same document. `pages@k` is the average
number of distinct pages in the top-k spans (cost proxy).

| Column | hit@1 | hit@3 | MRR | pages@1 | pages@3 |
|--------|-------|-------|-----|---------|---------|
| DocBot BM25, 512-token chunks | 0.722 | 0.917 | 0.815 | 5.00 | 13.42 |
| DocBot BM25, 64-token chunks | 0.611 | 0.806 | 0.708 | 1.31 | 3.11 |
| Tree navigator, lexical on titles | 0.083 | 0.139 | 0.102 | 0.94 | 3.56 |
| Tree navigator, lexical on titles + lead text | 0.222 | 0.222 | 0.222 | 1.39 | 3.19 |
| Hybrid (gating + tree-node arm) | 0.722 | 0.778 | 0.750 | 1.67 | 4.14 |

Reading: on these small fixtures the lexical tree navigator is far below BM25 chunk search on hit rate.
It is the LLM-free lower bound: it only sees node titles (and a 40-word lead as a stand-in for a summary),
and the questions are paraphrased, so title overlap is rare. It reads few pages per query, but that cost
advantage is worthless at these hit rates. These numbers do NOT measure an LLM agent walking the tree,
which is the intended consumer. Chunks are mapped to pages by matching chunk lines against page lines.
The DocBot chunker only carries pages for PDFs, so this mapping is the eval's own.

Hybrid column (PAGEINDEX.13, measured with the same command): `AdaptiveGating` with the memory layers empty
and only the tree-node arm on, `relevance_threshold` 0. The arm scores every tree node directly with BM25 over
title + summary (none in the fixtures) + the node's own page text, and returns the node with breadcrumb, page
range and node id; there is no top-down navigation and no vector leg (the store holds no node embeddings).
Test: `test_eval_hybrid_column_after_issue_13`.

Hybrid vs BM25 alone, plainly: on hit@3 the arm (0.778) is WORSE than DocBot 512-token chunks (0.917) and
slightly worse than 64-token chunks (0.806). On hit@1 it ties the 512-token chunks (0.722) and beats the 64-token
ones (0.611); MRR 0.750 sits between them (0.815 / 0.708). Its cost is 1.67 pages at top-1 and 4.14 at top-3
versus 5.00 / 13.42 for the 512-token chunks, so per page read it is the best column, but it does not beat chunk
BM25 on recall at k=3. Likely reasons, not verified by ablation: (1) nodes are coarse, so a node's BM25 length
normalisation dilutes a short answer sentence inside a long section, whereas 64-token chunks isolate it;
(2) the top 3 nodes often overlap or are neighbours in one section, so the k=3 list spends slots on the same
region; (3) the fixture questions are paraphrases and a lexical leg alone cannot bridge them. Fusing the arm
with DocBot chunks and adding a node-summary vector leg are the untested next steps. On 36 questions over 3
small documents, differences of one or two questions (0.028 each) are noise.

Optional modes (never in CI, both `#[ignore]`d):
- `test_eval_llm_navigator_optional` gives the outline to the LLM of `LlmAdapter::from_env()`
  (`DOCBOT_LLM_*`) and scores its chosen node: `cargo test -p xavier --features ci-safe --test pageindex_eval -- --ignored --nocapture llm`.
- `PAGEINDEX_OSS_BENCHMARK_DIR` (read by the test only, not by the crate or `.env.example`) points to a local
  checkout of VectifyAI/PageIndex-OSS-Benchmark converted to `<dir>/questions.jsonl` (fields `doc`, `query`|`question`,
  `start_page`|`start`, `end_page`|`end`) with `.md`/`.txt` sources in `<dir>/documents/`. The real repo ships
  `questions.json` and PDFs; its exact schema was not verified, so this loader expects the converted form and the
  PDF documents need the `pageindex-pdf` feature. Unset means skipped; nothing is downloaded.

## End-to-end evaluation (real PDFs)

Measured 2026-09-29 on the public PageIndex-OSS-Benchmark (github.com/VectifyAI/PageIndex-OSS-Benchmark): 62 lookup questions, 34 public PDFs, 1,945 pages. Harness: `scripts/pageindex-e2e/`.

**Setup.** xavier `feat/pageindex` release build, isolated instance, all 34 PDFs indexed over MCP (`/mcp/tools/call` `pageindex_index_document`), LLM-free tree (no summaries). The navigator is Claude Sonnet 5.5 subagents using only the pageindex MCP tools, blind to the answers. Correctness is a normalized string match plus manual review by the orchestrator.

**Ingest.** 34/34 PDFs, 12 via bookmarks, 22 via pdfium layout, about 10 s total.

| Run | Correct | Pages read / question | Calls / question | Evidence page read |
|-----|---------|-----------------------|------------------|--------------------|
| BM25 page baseline (no LLM) | hit@1 0.484, hit@3 0.629, hit@5 0.742 | - | - | - |
| Round 1: tree only | 61/62 | 11.58 | 4.03 | 0.855 |
| Round 2: tree + `pageindex_search` + PDF quality fixes | 60/62 | 1.71 | 3.02 | 0.742 |

Round 2 tool usage: `pageindex_search` 84, `get_page_content` 96, `get_document_structure` 3, `get_document` 4.

Wrong answers in round 2: q25 answered 7 (Mainland China data centers) vs reference 18 (global); q50 answered 496 (cash-flow statement) vs reference 495 (equity note), same in round 1.

Reference, PageIndex Python as published: gpt-5.6-luna high 60/62, gpt-5.6-terra medium 61/62, gpt-5.6-sol medium 62/62.

**Caveats.** Different navigator model than PageIndex's published runs; single run per round; the judge is the orchestrator (manual review), not an LLM judge.

**Reading.** An LLM navigating the LLM-free tree matches PageIndex's published accuracy (61/62). Adding in-document search cut pages read per question from 11.6 to 1.7 at 60/62. Lexical-only navigation without an LLM is weak (see "Eval results", fixture eval), so the value is the agent-facing tools plus the hybrid arm, not a replacement of BM25/vector retrieval.

## 11. Risks & notes

- pdfium provisioning (NixOS/CI) is the main external risk; contained by feature flags and runtime binding.
- `lopdf` text extraction is weaker than pdfium; bookmarks path may fall back to `pdf-layout` for page text when available.
- Legal regexes are duplicated from `legal_chunker.rs`; parity test guards drift.
- Cargo.lock / root `Cargo.toml` conflicts with concurrent waves: issue 01 and 06 are the only ones editing root `Cargo.toml`.
- Main CI has an intermittent, PRE-EXISTING failure in "Rust Integration (Observability Contract)" (#2701); it is not caused by this initiative.
- REQ numbers 080..084 chosen to avoid collision with REQ-066..075 reserved on `docs/wave-29-*` branches.
