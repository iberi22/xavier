# [Ola 4 · 07] EPIC cierre Ola 4: features.json 100% local-first + ROADMAP + devlog

> **LAST issue of Ola 4.** Do not implement until issues 01–06 are merged.

## Web Research Required

None (repo reconciliation only).

## Exact Technical Context

- Update `.gitcore/features.json`:
  - `feat-local-first.progress_pct` → **100** if hot-swap API+UI + headless fallback landed
  - `feat-runtime-health` → **100** if usage UI optional; keep 98–100 honest
  - `metadata.overall_progress_pct` = mean of features
  - `last_verified` = today
- Update `.gitcore/features-detailed.json` Ola 4 sub_features to completed
- `docs/ROADMAP_LOCAL_FIRST.md`: Ola 4 → DONE
- New devlog `docs/devlog/YYYY-MM-DD-ola4-close.md`
- Comment on GitHub #522 with final % 

## Problem

Need formal wave closure and accurate feature tracking.

## Acceptance Criteria

- [ ] features.json valid JSON
- [ ] feat-local-first reflects reality (100 only if 01+02+04 done; else 98 with note)
- [ ] ROADMAP Ola 4 DONE table
- [ ] Devlog written
- [ ] `python -c "import json; json.load(open('.gitcore/features.json'))"`

## Files to Modify

| File | Change |
|---|---|
| `.gitcore/features.json` | Percents + notes |
| `.gitcore/features-detailed.json` | Ola 4 subs |
| `docs/ROADMAP_LOCAL_FIRST.md` | Ola 4 DONE |
| `docs/devlog/*-ola4-close.md` (NEW) | Devlog |

**DO NOT touch:** Rust sources

## Dependencies and Merge Order

- **Depends on:** ALL prior Ola 4 issues merged
- **Always last**
