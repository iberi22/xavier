# PageIndex: tree-based document retrieval

Xavier can index a long document as a hierarchical table-of-contents tree and let
the calling agent navigate it, reading only the pages it needs. No chunking, no
embeddings, no LLM inside Xavier.

Status: `stable` (ledger `feat-pageindex-tree-core`, `feat-pageindex-pdf`,
`feat-pageindex-hybrid`). Decision record: [ADR-036](../adr/ADR-036-tree-reasoning-retrieval.md).
Full spec: [FEATURE-pageindex-tree-retrieval](specs/FEATURE-pageindex-tree-retrieval.md).

## Inspiration and attribution

This feature is inspired by [PageIndex](https://github.com/VectifyAI/PageIndex) by
Vectify AI (MIT license, Copyright (c) 2025 Vectify AI) and its public
[PageIndex-OSS-Benchmark](https://github.com/VectifyAI/PageIndex-OSS-Benchmark).

What was taken:

- **The idea and retrieval design**: a hierarchical ToC tree per document (title,
  node id, page range, optional summary, children), navigated by an LLM with a few
  read tools instead of chunk similarity. Upstream's pitch is "similarity is not
  relevance"; we agree.
- **The shape of the read tools**: browse documents, get document, get document
  structure, get page content.
- **Heuristics studied** in upstream's "flash" layout parser, tree optimizer and
  markdown indexer (font-size/weight heading detection, merging tiny nodes,
  splitting huge ones, heading nesting).

What was not taken: code. Everything is reimplemented from scratch in Rust in
`crates/xavier-pageindex`. There is no Python dependency and upstream is not
vendored. Xavier is AGPL-3.0-only; PageIndex remains MIT under its own copyright.

## Why we ported it

- **Similarity is not relevance.** Vector RAG returns the passages that look most
  like the question, not the section that answers it.
- **Chunking destroys structure.** In long structured documents (10-Ks, contracts,
  manuals, papers), a fragment loses its section, its neighbours and its page.
- **Agents need traceable answers.** A tree gives node ids, page ranges and
  breadcrumbs, so an answer can cite "section 4.2, pages 31-33".
- **Local-first and private.** The index builds with no LLM and no network; documents
  never leave the machine.
- **The reasoning is already there.** Xavier's callers (Claude, Codex, agy, ...) are
  LLM agents. The tree is navigated by the calling agent over MCP, so Xavier adds no
  model cost and needs no query-time LLM.

## Why in Xavier, and why Rust

Alternatives weighed in ADR-036: a separate app (another daemon, auth and storage to
duplicate), a Go service wrapping the Python logic (second toolchain, no CI story),
copying the Python package (runtime dependency, LLM hard-wired into indexing, not
local-first), staying vector-only (no exact-section navigation, cannot show
structure), and code inside the main crate (blocks reuse, slows CI).

Chosen: an independent workspace crate with no dependency on the `xavier` crate,
plus thin glue in Xavier (MCP tools, HTTP routes, one optional retrieval arm). It
shares the runtime's auth, roles and deployment, and can be reused elsewhere.

## What you get

### Tools

Available over MCP (prefix `pageindex_`) and HTTP (`/v1/pageindex/...`, same auth as
the rest of the API).

| Tool | Purpose | Key params |
|------|---------|------------|
| `pageindex_browse_documents` | List indexed documents, optionally ranked by a query | `query`, `limit`, `offset` |
| `pageindex_get_document` | Metadata: page count, builder, whether to read the structure first | `doc_name` |
| `pageindex_get_document_structure` | The ToC tree (no page text), depth- and size-bounded | `doc_name`, `max_depth`, `node_id` |
| `pageindex_get_page_content` | Text of specific pages (max 20 per call) | `doc_name`, `pages` (e.g. `"5-7,12"`) |
| `pageindex_search` | In-document search: pages with node id, breadcrumb, score, snippet | `doc_name`, `query`, `limit` |
| `pageindex_index_document` | Index a document (write role required) | `doc_name`, `path` or `content`, `format`, `summarize`, `optimize` |

Routes: `GET/POST /v1/pageindex/documents`, `GET|DELETE /v1/pageindex/documents/{name}`,
and `.../structure`, `.../pages`, `.../search` under it.

### Capabilities

- **Formats**: markdown, plain text, legal text (chapter/article/clause), and PDF.
- **PDF cascade**: bookmarks first (pure Rust, `lopdf`); then layout analysis with
  pdfium (font size/weight headings, running headers and page numbers dropped); then,
  optionally, an LLM-generated ToC; last resort, titled fixed windows. PDFs with an
  empty user password open normally. The tree records which builder produced it.
- **Search inside a document**: BM25 over pages plus node titles and summaries.
  Snippets only, never whole pages, so the agent can jump straight to the right pages.
- **Traceability**: every result carries node id, page range and breadcrumb.
- **Cost**: the agent reads the structure and then only the pages it needs.
- **Optional hybrid arm**: tree nodes can join Xavier's normal retrieval as one more
  ranked source fused by RRF. Off by default; see the evidence below for what it does
  and does not buy you.
- **Reusable crate**: `crates/xavier-pageindex` has no dependency on Xavier.

## Validation

We ran two evaluations, and they say different things. Both are reproducible.

### End-to-end on real PDFs

Data: the public PageIndex-OSS-Benchmark, 62 lookup questions over 34 PDFs (1,945
pages), measured 2026-09-29. All 34 PDFs indexed over MCP (12 via bookmarks, 22 via
pdfium layout, about 10 s total) into an isolated Xavier instance, with an LLM-free
tree (no summaries). The navigator was Claude Sonnet 5.5 subagents using only the
`pageindex_*` tools, blind to the answers. Correctness is a normalized string match
plus manual review.

| Run | Correct | Pages read / question | Calls / question |
|-----|---------|-----------------------|------------------|
| BM25 page baseline (no LLM) | hit@1 0.484, hit@3 0.629, hit@5 0.742 | - | - |
| Round 1: tree only | 61/62 | 11.58 | 4.03 |
| Round 2: tree + `pageindex_search` + PDF quality fixes | 60/62 | 1.71 | 3.02 |

For reference, PageIndex (Python) as published by its authors: 60/62
(gpt-5.6-luna, high), 61/62 (gpt-5.6-terra, medium), 62/62 (gpt-5.6-sol, medium).

Reading:

- An LLM navigating Xavier's LLM-free tree reaches the accuracy PageIndex publishes
  (61/62), so the port preserves what makes the approach work.
- Adding in-document search cut the pages read per question from 11.6 to 1.7 at
  60/62. That is the cost argument: about 7x fewer pages for one question missed.
- The two misses in round 2 are ambiguous-scope answers (q25: mainland-China data
  centers instead of global; q50: cash-flow statement instead of the equity note,
  wrong in round 1 too).

Caveats: the navigator model differs from PageIndex's published runs, each round is
a single run, and the judge is manual review rather than an LLM judge. Treat the
comparison with the published figures as indicative, not a controlled benchmark.

### Fixture evaluation (no LLM)

Data: 3 committed documents, 36 questions with gold page ranges, no LLM, no network
(`cargo test -p xavier --features ci-safe --test pageindex_eval -- --nocapture`).

| Column | hit@1 | hit@3 | MRR | pages@3 |
|--------|-------|-------|-----|---------|
| BM25, 512-token chunks | 0.722 | 0.917 | 0.815 | 13.42 |
| BM25, 64-token chunks | 0.611 | 0.806 | 0.708 | 3.11 |
| Lexical tree navigator (titles) | 0.083 | 0.139 | 0.102 | 3.56 |
| Hybrid arm (gating + tree nodes) | 0.722 | 0.778 | 0.750 | 4.14 |

Honest limitation: without an LLM, lexical navigation of the tree is weak, and the
hybrid arm does not beat chunk BM25 on recall at k=3. Its value is precise sections
with breadcrumb and page range at low read cost (1.67 pages at top-1 versus 5.00).
The LLM-free navigator is a lower bound, not a measurement of an agent. At 36
questions, a one- or two-question difference (0.028 each) is noise.

### Conclusion

PageIndex complements BM25 and vector retrieval; it does not replace them. Use
BM25/vector search to find which document or memory matters, and the tree tools
when you need the exact, citable section of a long structured document. The hybrid
arm stays off by default until its vector leg is measured.

Reproduce: [`scripts/pageindex-e2e/`](../../scripts/pageindex-e2e/README.md) (real
PDFs) and the command above (fixtures).

## How to use

### Build features

| Cargo feature | Effect |
|---------------|--------|
| `pageindex` | Markdown/text/legal trees, store, tools. On by default (also in `ci-safe`); no native dependency |
| `pageindex-pdf` | Adds PDF bookmarks and per-page text (pure Rust) |
| `pageindex-pdfium` | Adds pdfium layout analysis for PDFs without bookmarks. Needs `libpdfium` at runtime |

```bash
cargo build --release --features pageindex-pdfium
```

### Configuration

Set in the environment (see `.env.example`):

| Variable | Default | Purpose |
|----------|---------|---------|
| `XAVIER_PAGEINDEX_DB` | `$XAVIER_DATA_DIR/pageindex.sqlite3` | Tree store (own SQLite file) |
| `XAVIER_PAGEINDEX_INGEST_ROOTS` | empty | Directories `path` ingest may read; empty means inline `content` only |
| `XAVIER_PAGEINDEX_PDFIUM_LIB` | unset | Directory or file of `libpdfium`; falls back to `PDFIUM_DYNAMIC_LIB_PATH` |
| `XAVIER_PAGEINDEX_ARM_ENABLED` | `false` | Add the tree-node arm to retrieval |
| `XAVIER_PAGEINDEX_ARM_WEIGHT` | `1.0` | RRF weight of that arm |
| `XAVIER_PAGEINDEX_SUMMARIZE` | `false` | Summarize nodes on ingest (uses the `DOCBOT_LLM_*` backend) |
| `XAVIER_PAGEINDEX_LINES_PER_PAGE` | `60` | Virtual page size for text/markdown |
| `XAVIER_PAGEINDEX_MAX_RESPONSE_CHARS` | `95000` | Per-tool response cap |
| `XAVIER_PAGEINDEX_MIN_NODE_TOKENS` / `MAX_NODE_TOKENS` | `200` / `6000` | Merge / split thresholds |
| `XAVIER_PAGEINDEX_SUMMARY_MAX_WORDS` | `60` | Summary length |

### Example

An agent indexing and querying a PDF over MCP:

```text
pageindex_index_document  {"doc_name": "acme-10k", "path": "/data/docs/acme-10k.pdf"}
pageindex_get_document_structure {"doc_name": "acme-10k", "max_depth": 2}
pageindex_search          {"doc_name": "acme-10k", "query": "data center count"}
pageindex_get_page_content {"doc_name": "acme-10k", "pages": "41-42"}
```

The `path` must sit under `XAVIER_PAGEINDEX_INGEST_ROOTS`. Re-indexing identical
content is a no-op.

### NixOS: pdfium

`shell.nix` adds nixpkgs `pdfium-binaries` and sets `XAVIER_PAGEINDEX_PDFIUM_LIB` to
its `lib` directory. Xavier binds pdfium at runtime and needs API >= 6996; the
nixpkgs package (7087 when checked) qualifies. For a systemd unit, set the same
variable. Without pdfium, PDFs still index through bookmarks or fixed windows.

## Limitations and roadmap

- Scanned pages have no text layer; they need OCR, which is out of scope.
- Heading detection is heuristic: table-heavy PDFs can produce noisy headings.
- Page numbers are physical page indexes; printed page labels are not exposed.
- The hybrid arm has a BM25 leg only. A node-summary vector leg is planned as
  follow-up issue 15
  ([.gitcore/waves/wave-pageindex/issue-15-followup-vector-leg.md](../../.gitcore/waves/wave-pageindex/issue-15-followup-vector-leg.md));
  it ships on only if it beats the BM25-only arm on the fixtures.
- Images and table extraction are out of scope.
