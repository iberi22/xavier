# FEATURE: Skill Wave Ledger, SRS and Ack-Gate Docs

**Status:** `planned` | **Score:** — | **Last Tested:** —

## Overview
Closing feature for the wave: the ledger entries reference REQ-060..065 which do not exist yet in `docs/SRS/REQUIREMENTS.md` (max REQ-059), the six specs need final measured numbers, and the ack-gate predicate needs one canonical home. This issue is docs-only (`src/` untouched) and closes the wave on a green `verify-pipeline` run — statuses are promoted by the pipeline, never by hand (AGENTS.md §4-5).

## Architecture & Design
- REQ-060..065 appended in IEEE 830 reduced style, each citing merged `file:line` evidence from #301–#305.
- Specs finalized with measured Recall@3/MRR, live `GET /skills` counts, dispatch samples.
- Ack-gate predicate documented where #304's code references it.
- `reconciliation_notes` history is append-only.

## Implementation Paths
- `docs/SRS/REQUIREMENTS.md` (REQ-060..065)
- `docs/features/specs/FEATURE-feat-skill-*.md` (finalize)
- Ack-gate canonical doc (location per #304)

## Sub-features
- **SRS traceability:** 6 REQs, every citation `test -f`-verifiable.
- **Spec finalization:** Status + measurements + live evidence.
- **Wave close:** pipeline run recorded, ledger matches verdict.

## Test References
- `test_ledger_skill_entries_valid` (ledger schema gate) + pipeline exit 0 + `rg -c "^## REQ-"` delta of +6.

## Known Issues & Notes
- Merges last (#306). No forward references: REQs cite merged code, not issue text.
