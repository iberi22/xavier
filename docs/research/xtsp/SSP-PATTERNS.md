# SWAL Stability Protocol (SSP) Design Patterns

The SWAL Stability Protocol (SSP) defines a standardized, high-performance, and privacy-preserving protocol for tracking codebase stability, indexing feature implementation progress, and dynamically assembling high-density, compact context triggers for autonomous agents.

## 1. Canonical Namespaces and Kinds

To avoid catalog fragmentation and coordinate indexing pipelines across multiple SWAL repositories, the protocol establishes two canonical `kind` fields and their corresponding `path` patterns:

- **`stability_report`**: Maps stability status reports for entire repositories or releases.
  - **Canonical Path**: `stability/{repo}/latest` or `stability/{repo}/{tag}`
  - **Payload Structure**: Contains stability metric scores (Functionality, Security, Overall) and automated verification verdicts.

- **`feature_snippet`**: Contains compact, high-density metadata summaries of individual features parsed from `.gitcore/features.json`.
  - **Canonical Path**: `features/{repo}/{feature_id}`
  - **Payload Structure**: Contains progress percentage (`%real`), implementation status, last tested date, and associated tests/requirements (<300 characters).

## 2. PathExact Deduplication & UPSERT Strategy

To prevent database inflation and in-memory cache duplication during continuous execution loops (e.g. repeated runs of `stabilize.sh` or `stabilize-index`), the system enforces **PathExact Deduplication** automatically for these namespaces.

Even when general workspace deduplication is disabled (`dedup.enabled = false`):
1. Any memory write to `stability/*` or `features/*` resolves to a `PathExact` scope lookup.
2. The database retrieves the existing `id` matching that path, and performs a native `INSERT OR REPLACE` (UPSERT).
3. The `QmdMemory` in-memory document list intercepts these canonical paths and updates the existing record in-place instead of pushing duplicates.

This ensures exactly **one record per path** is maintained.

## 3. Context Assemble Trigger (`POST /v1/context/assemble`)

When an agent initiates a task, compiling the entire codebase or full-text memory logs creates heavy token overhead and dilutes context. The `POST /v1/context/assemble` endpoint solves this via a **compact fat-search trigger**:

1. **Query-Aware Filtering**: Searches memories based on terms of the issue/task (e.g. "rate limiter").
2. **Preference for Canonical SSP & Decisions**: Heavily prioritizes matching `feature_snippet`, `stability_report`, and `decision` memory kinds.
3. **Query-Centered Snippeting**: Extracts a tight, centered excerpt around key query terms.
4. **Compact Response**: Returns a concatenated JSON payload guaranteed to be under **2KB**, delivering **~80% token savings** compared to raw context retrieval.

---
*SWAL Stability Protocol (SSP) - Wave 19 Specification*
