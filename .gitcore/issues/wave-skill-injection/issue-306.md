# [Ola skill-injection] feat-skill-ledger-docs — SRS, Specs, Ack-Gate Docs, Wave Close

> Ola skill-injection — Local-first intelligent skill injection, 100% offline (no Jev API).
> Labels: `wave-skill-injection`

---

## PR Delivery Requirements (ANTI-EMPTY-PR)
- [ ] `git status --porcelain` muestra los archivos nuevos/modificados ANTES de abrir el PR
- [ ] `git diff --stat HEAD` lista los archivos (NO vacío)
- [ ] El PR DEBE contener ≥1 archivo: verificar con `git ls-files` antes de push

## Current State (MEDIBLE)
- `docs/SRS/REQUIREMENTS.md` — max REQ is REQ-059; zero skill-injection requirements. Ledger entries `feat-skill-*` reference REQ-060..065 (dangling until this issue lands).
- `docs/features/specs/FEATURE-feat-skill-*.md` — 6 planned stubs from wave setup (this wave); need final status + measured numbers (Recall@3, MRR, live counts).
- Ack-gate predicate (issue #304) needs canonical documentation so future callers reuse one definition.
- Wave-close rule (AGENTS.md §4-5): green `verify-pipeline` run promotes; never hand-promote.

## Desired State (DELTA)
- **Update**: `docs/SRS/REQUIREMENTS.md` — append REQ-060..065 (scan paths, semantic rank, loader fate, fusion gates, MCP tools, offline mandate), each with concrete code evidence (`file:line` references to the merged implementation, IEEE 830 reduced style per repo).
- **Update**: the 6 `FEATURE-feat-skill-*.md` specs — final Status, measured eval numbers, live endpoint evidence (`GET /skills` count, dispatch sample).
- **New/Update**: canonical ack-gate doc (predicate + rationale + 0-ms path) where the codebase references it.
- **Run**: full `scripts/verify-pipeline.sh` (and strict if green) and record the wave-close result in the issue/PR. Promote ledger statuses ONLY via green runs.

## 🌐 Web Research Required
**MANDATORY — 4-6 queries.**
1. search: "IEEE 830 software requirements specification traceability code"
2. search: "SRS requirements code evidence file line references"
3. search: "GitCore features.json reconciliation notes wave close practice"

## 🔬 Agent Session Prompt
"Before implementing, please:
1. Read the merged diffs of #301–#305 (or their final state) — every REQ must cite real `file:line` evidence, never planned paths.
2. Read `docs/SRS/REQUIREMENTS.md` head + one full REQ entry to mirror format.
3. Run `scripts/verify-pipeline.sh` yourself; paste the score line."

## Existing Code Patterns (DEBES seguir estos)
- `docs/SRS/REQUIREMENTS.md` → `## REQ-NNN: Title` + evidence style.
- `.gitcore/features.json` → promotion only by pipeline (never edit `status` upward by hand).

## Acceptance Criteria (VERIFICABLES POR COMANDO)
- [ ] `rg -c "^## REQ-" docs/SRS/REQUIREMENTS.md` increased by 6 (REQ-060..065 present)
- [ ] Every REQ cites ≥1 real `path:line` that exists (`test -f` passes for each cited file)
- [ ] `scripts/verify-pipeline.sh` — exit 0; score line pasted in PR; ledger statuses match pipeline verdict (planned→promoted only on green)
- [ ] `git status --porcelain` shows only docs/ledger intends — zero `src/` changes in this PR

## Files to Modify
| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `docs/SRS/REQUIREMENTS.md` | Exists, max REQ-059 | Append REQ-060..065 | LOW |
| `docs/features/specs/FEATURE-feat-skill-*.md` | Planned stubs | Finalize with measurements | LOW |
| Ack-gate doc | TBD by #304 | Canonical write-up | LOW |

## DO NOT touch (Anti-Regression)
- `src/` — docs-only PR (except doc-comments if a predicate moved; justify).
- Ledger `status` fields by hand — pipeline promotes.
- `reconciliation_notes` history entries (append-only if touched at all).

## Anti-Hallucination Guard ⚠️
1. **Evidence or it didn't happen**: every measurement (Recall@3, counts, latencies) re-run by implementer, commands quoted.
2. **No forward references**: REQs cite merged code, not issue text.

## Verification
```bash
scripts/verify-pipeline.sh
rg -c "^## REQ-" docs/SRS/REQUIREMENTS.md
# for each cited path:line → test -f + sed -n '<line>p'
```

## Dependencies & Merge Order
- **Depends on:** #301–#305 merged
- **Merge order within wave:** 6 (closes wave)
- **Expected effort:** Medium 2h
