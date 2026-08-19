# ADR-009: Codegraph to Maturity and Documentation Bridge

*Status: ACCEPTED | Date: 2026-07-29*

---

## Context

Xavier implements an Abstract Syntax Tree (AST)-backed code indexing and symbol search capability through the `code-graph` sidecar. However, several downstream systems suffered from decoupling, leading to sub-optimal reliability and configuration drift:
1. **Maturity Scanning System (Layer 1)**: The maturity scanner is designed to statically verify the presence of critical feature anchors (symbols) across the codebase. Previously, it expected a portable JSON dump (`.xavier/codegraph.json`) to be manually present, falling back to a slow multi-file grep if missing.
2. **Auto-docs Generator (`xavier chronicle`)**: The documentation harvester (`AutoDocsGenerator`) struggled with path misalignment, historically expecting the SQLite database at `data/code_graph.db` rather than the active server's location in `.xavier/code_graph.db`.
3. **Absence of Portability**: Generating the `.xavier/codegraph.json` dump required manual invocation, leaving workspaces without the portable dump and triggering grep fallback.

---

## Decision

We establish a robust, synchronized bridge between the Code Graph indexer, the Maturity Scanner, and the Auto-Docs tooling:

1. **Auto-dump after Indexing**: Every successful index or scan operation (triggered via CLI or HTTP API) automatically invokes a soft-fail `perform_dump` process to synchronize `.xavier/codegraph.json`. If writing fails, it handles the error gracefully without breaking the primary indexing success.
2. **Unified Path Management**: All code graph components dynamically resolve workspace-relative paths for both `.xavier/code_graph.db` and `.xavier/codegraph.json` via shared path helpers inside `src/codebase/codegraph_paths.rs` (`code_graph_db_path_for` and `codegraph_dump_path_for`).
3. **Layered Maturity Resolution Chain**: The Layer 1 Maturity Scanner checks symbols through an ordered, non-panicking chain of fallback mechanisms:
   - **SQLite Database**: Directly query the live database (if total symbols > 0).
   - **JSON Dump**: Parse `.xavier/codegraph.json` (if present and has parsed symbols > 0).
   - **Grep Fallback**: Run a fast multi-file regex/substring search over standard source directories as a last-resort graceful degradation.
4. **Chronicle Path Realignment**: Realized `AutoDocsConfig` and the chronicle CLI defaults to delegate path resolution to the shared helper, ensuring complete synchrony between the documentation generator and the active index.

---

## Rationale

- **No Manual Intervention**: Automating the JSON dump generation ensures that the portable state needed for lightweight scans is always fresh.
- **Robust Fail-Safe**: Prioritizing direct SQLite database queries avoids unnecessary file I/O and substring matching overhead, while keeping the JSON dump and grep fallback as robust buffers guarantees that the maturity engine never panics or stalls, even under empty-state or read-only environments.
- **Unified CWD and Workspace Paths**: Shared path helpers eliminate hardcoded path mismatches across CLI, daemon, and tooling processes.

---

## Consequences

**Positive:**
- Increased maturity scanner speed (usually <10ms when querying SQLite index vs. up to 5s using regex fallback).
- Guaranteed presence of the `.xavier/codegraph.json` dump file for external analytics or offline workflows.
- Auto-docs generation works out-of-the-box using the standard active workspace database.
- Completely soft-fail operations that preserve primary index task completion even if disk write operations on the dump file fail (e.g., due to OOM or duplicate permissions issues).

**Negative:**
- Marginally increased scan duration (minimal, run in non-blocking thread) due to serialization and compression of code graph records.
