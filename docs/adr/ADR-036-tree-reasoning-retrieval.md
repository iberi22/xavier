# ADR-036 — Tree-reasoning retrieval (PageIndex idea) as an independent crate

| Campo | Valor |
|-------|--------|
| **ID** | ADR-036 |
| **Estado** | Aceptado (2026-09-29, wave-pageindex closed) |
| **Fecha** | 2026-09-28 |
| **Autores** | Claude Code (F0 design agent), owner review pending |
| **Relacionados** | `docs/features/specs/FEATURE-pageindex-tree-retrieval.md`, REQ-080..084, `.gitcore/waves/wave-pageindex/`, ADR-019 (plugin-first boundary), ADR-034 (skill controller) |

## Context

Xavier retrieves by similarity: BM25 + `sqlite-vec` embeddings fused with RRF
(`src/retrieval/gating.rs`, `src/search/rrf.rs`) and the DocBot pipeline
(`src/rag/pipeline.rs`, `src/collections/*`). Long structured documents (contracts,
manuals, reports, specs) lose their structure when chunked: a question such as
"what does the termination clause say about notice periods?" needs the exact
section, and similarity chunks return fragments without the surrounding hierarchy.

VectifyAI/PageIndex (MIT) shows an alternative, "vectorless reasoning-based RAG":

1. **Index** = a hierarchical table-of-contents tree per document. Each node has
   `title`, `node_id`, `start_index`/`end_index` (page range), an optional
   `summary`, and `nodes` (children). Built from PDF bookmarks, layout heuristics
   (`pageindex/flash`, pdfium char-level, no LLM), an LLM fallback
   (`page_index_classic.py`), or markdown headings (`page_index_md.py`);
   `tree_optimize.py` merges/splits nodes.
2. **Retrieve** = an LLM agent navigates the tree using four read tools
   (`pageindex/agent_tools.py`): `browse_documents`, `get_document`,
   `get_document_structure`, `get_page_content`.

Constraints in force:

- Xavier is local-first and privacy-preserving (`.gitcore/docs/SWAL_GOAL.md`): no
  mandatory cloud LLM, no mandatory native library in default/`ci-safe` builds.
- Xavier's MCP clients (Claude, Codex, agy) are already capable reasoning agents.
  Xavier should expose data and let the caller reason; it must not need an LLM at
  query time.
- `src/documents/pdf.rs` is a naive `BT/ET` extractor (no fonts, no positions), so
  it cannot detect headings by layout. `src/documents/legal_chunker.rs` already
  produces breadcrumb hierarchies for legal text (regex table for
  Capitulo/Articulo/Clausula/Parrafo).
- Decided with the owner: port the IDEA to Rust, not copy the Python code.

## Alternatives

| Opcion | Descripcion | Coste/complejidad esperada |
|--------|-------------|----------------------------|
| A | **Independent workspace crate `crates/xavier-pageindex`** (no dependency on the `xavier` crate), consumed by Xavier as MCP tools + HTTP routes + one extra retrieval arm | Medium. New crate, thin glue in Xavier. Reusable as standalone binary later. |
| B | Separate app under `apps/` (own repo, own process, own port) | High. Another daemon to deploy and auth; duplicates storage/token handling; ADR-019 favours in-tree plugins for retrieval capabilities. |
| C | Go (or other-language) service wrapping the Python logic | High. Second toolchain in a Rust-only repo, IPC and packaging cost, no NixOS/CI story. |
| D | Copy/vendor the Python package and shell out | Medium now, high later. Python runtime dependency, LLM calls hard-wired into indexing, licence and maintenance burden, not local-first. |
| E | Vector-only (status quo): improve chunking + hierarchical breadcrumbs in the existing store | Low. Does not give exact-section navigation, and needs re-embedding for every tweak; cannot answer "show me the structure". |
| F | Implement inside the `xavier` crate (`src/pageindex/`) | Low-medium. Simple, but couples the tree logic to the whole runtime, blocks standalone reuse, and slows CI iteration (huge crate). |

## Simulation (OBLIGATORIO)

- **Estado:** NO EJECUTADA. The `swal-sim` engine (`adr_sim.py`) was not run in the F0
  design phase; this ADR was `Propuesto` at design time and became `Aceptado` when the wave closed (2026-09-29).
- **Nota de aceptacion:** the simulation was never run; acceptance rests on the measured
  end-to-end evaluation on real PDFs (see "Verificacion posterior"), not on a simulated score.
- **Sensitivity:** unknown until run.

## Supuestos y su anclaje

| Parametro | Valor usado | Fuente |
|-----------|-------------|--------|
| Calling agent does the tree reasoning | yes | **ASSUMPTION** (owner decision 2026-09-28) |
| PageIndex MIT licence permits porting the idea | yes; no code copied | PageIndex repository LICENSE (MIT) |
| Node schema fields (`title,node_id,start/end,summary,nodes`) | as PageIndex | `pageindex/page_index_md.py`, `pageindex/agent_tools.py` |
| Default build must not need a native PDF library | required | `AGENTS.md` sec. 2 (`ci-safe`), `.github/workflows/ci.yml:89-92` |
| `pdfium-binaries` packaged in nixpkgs | available (7087 at time of check) | `nix eval nixpkgs#pdfium-binaries.version` |
| Retrieval hit-rate gain of tree vs hybrid | unknown | **ASSUMPTION**, to be measured in F3 |

## Decision

Adopt **Option A**. Build `crates/xavier-pageindex`, an independent workspace crate
(no `xavier` dependency; traits for `Summarizer`), which:

- models documents as hierarchical page/section trees, buildable **with no LLM**
  (markdown headings, plain-text numbered headings, legal patterns, PDF bookmarks,
  PDF layout heuristics), with an LLM only as optional enrichment (node summaries,
  ToC fallback);
- stores trees in its own SQLite file (WAL), keyed by workspace, so Xavier core
  migrations are not touched;
- keeps PDF behind cargo features: `pdf-outline` (pure Rust, `lopdf`) and
  `pdf-layout` (`pdfium-render`, dynamic binding at runtime, never linked at build
  time). Default and `ci-safe` builds need no native library.

Xavier consumes it in two ways:

1. **MCP tools + HTTP routes** (five read tools: four mirroring PageIndex plus `pageindex_search`,
   and one role-gated ingest tool), so the calling agent navigates the tree. Xavier needs
   no LLM at query time. MCP code lives in its own module
   (`src/server/mcp/tools_pageindex.rs`) with a single, late, isolated registration.
2. **A retrieval arm** (`src/retrieval/pageindex_arm.rs`) fused by RRF in the gating
   layer, off by default: node summaries are embedded into Xavier's store, BM25 +
   vectors select candidate documents/nodes, tree structure yields the exact page
   range.

## Consequences

- **Positivas:** exact-section retrieval with page ranges; structure is visible to
  the agent; works with zero LLM and zero network; the crate is reusable and can
  become a standalone binary; PDF support is opt-in so CI is unaffected.
- **Negativas / coste:** a new crate and a second SQLite file to back up and
  migrate; layout-heuristic PDF headings are approximate and will need tuning;
  pdfium must be provisioned by the operator (`PDFIUM_DYNAMIC_LIB_PATH`) on NixOS
  and in any CI job that enables `pdf-layout`; legal-pattern logic is ported (not
  shared) from `legal_chunker.rs`, so drift needs a parity test; more MCP tools
  increase the tool-list size for clients.
- **Que invalidaria esta decision:** F3 eval showing the tree arm does not beat or
  complement DocBot/hybrid on page-range hit-rate at k<=3 on both the in-repo fixtures
  and the PageIndex-OSS-Benchmark subset; or pdfium provisioning proving
  unworkable on the supported platforms (then keep only `pdf-outline`).

- **Follow-up:** the hybrid arm ships BM25-only; the node-summary vector leg is planned in
  `.gitcore/waves/wave-pageindex/issue-15-followup-vector-leg.md`.

## Verificacion posterior

- Metric: page-range hit-rate@k and MRR, tree vs DocBot vs hybrid (F3 eval,
  `tests/pageindex_eval.rs`); LLM-free.
- Gate: `scripts/verify-pipeline.sh` runs the three ledger entries
  (`feat-pageindex-tree-core`, `feat-pageindex-pdf`, `feat-pageindex-hybrid`).
- Review date: 2026-11-30, or when F3 closes, whichever is first.
- F3 eval, first measurement (2026-09-29, PAGEINDEX.14; hybrid column filled by PAGEINDEX.13 below):
  on 36 fixture questions the LLM-free lexical tree navigator scores hit@3 0.139 (titles) / 0.222
  (titles + lead text) against 0.917 / 0.806 for DocBot BM25 (512 / 64-token chunks). This is a
  lower bound, not an LLM agent, so it neither confirms nor triggers the invalidation criteria
  above; the decision must be re-reviewed with the hybrid column and the real-LLM run.
  Details: spec `FEATURE-pageindex-tree-retrieval.md`, "Eval results".
- F3 eval, hybrid column (2026-09-29, PAGEINDEX.13, measured, LLM-free, 36 questions): gating with the
  tree-node arm (BM25 over node title + summary + own text, no top-down navigation, no vector leg) scores
  hit@1 0.722, hit@3 0.778, MRR 0.750, pages@1 1.67, pages@3 4.14. That is worse than DocBot BM25 512-token
  chunks on hit@3 (0.917) and MRR (0.815), equal on hit@1 (0.722) while reading about 3x fewer pages at
  top-1 and top-3, and slightly below 64-token chunks on hit@3 (0.806). The arm does not beat chunk BM25 on
  recall, so the "beat or complement" criterion is not met on these fixtures; its value is precise sections
  with breadcrumb and page range at low read cost. The arm ships OFF by default
  (`XAVIER_PAGEINDEX_ARM_ENABLED`). Still open: fusion with DocBot chunks, a node-summary vector leg, and
  the real-LLM run.
- E2E on real PDFs (2026-09-29, measured): see below. Key finding: an LLM navigating the LLM-free tree matches
  PageIndex's published accuracy (61/62), and adding in-document search cut pages read per question from 11.6 to
  1.7 at 60/62. Lexical-only tree navigation without an LLM is weak (fixture eval), so the value is the
  agent-facing tools plus the hybrid arm, not a replacement of BM25/vector retrieval.

### End-to-end evaluation (real PDFs)

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

- Note: a flaky CI job, "Rust Integration (Observability Contract)" (#2701), is
  pre-existing on main and unrelated to this initiative.
