# [Ola 5v2 · 14] EPIC close: recompute feature % + update #496 / #478 / #115

> **LAST issue of Ola 5v2.** Do not start until 01–13 are merged or explicitly waived.
> **No `jules` label until orchestrator decides** (prefer human/orchestrator close).

## Web Research Required (Jules must search the web)

1. **Aggregating feature completion %** — search: `weighted feature completion percentage product management 2024`.
2. Optional: read `.gitcore/features.json` schema conventions already used in-repo.

## Exact Technical Context

- `.gitcore/features.json` — update:
  - `feat-token-savings`
  - `feat-code-graph-index` (if FTS5 landed)
  - `feat-plugin-system`
  - `feat-security-hygiene`
  - `feat-mcp-server`
  - `metadata.overall_progress_pct` = mean of all features
- `.gitcore/features-detailed.json` — flip sub_features to completed with evidence PR numbers
- New devlog: `docs/devlog/YYYY-MM-DD-ola5v2-close.md`
- Comments on #496, #478, #115 with checklist of what shipped

> CRITICAL: Docs/JSON only. No drive-by Rust. NEVER `.patch` files.

## Problem

Need formal wave closure with honest percentages after Jules PRs land.

## Acceptance Criteria

- [ ] Valid JSON for features files
- [ ] overall_progress_pct recalculated
- [ ] Devlog written
- [ ] Parent issues commented

## Files to Modify

| File | Change |
|---|---|
| `.gitcore/features.json` | percents + notes |
| `.gitcore/features-detailed.json` | sub_features |
| `docs/devlog/*ola5v2*` (NEW) | devlog |

## Verification

```bash
python -c "import json; json.load(open('.gitcore/features.json')); json.load(open('.gitcore/features-detailed.json'))"
```

## Dependencies and Merge Order

- **Depends on:** ALL prior Ola 5v2 issues
- **Always last**
