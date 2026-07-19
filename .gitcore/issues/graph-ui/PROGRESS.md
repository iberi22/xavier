# Ola Graph Explorer — Progress Tracker

**Wave closed:** 2026-07-18  
**Status:** CODE COMPLETE on `main`

## Merged PRs (order)

| PR | Title | Role |
|----|-------|------|
| [#662](https://github.com/iberi22/xavier/pull/662) | Palette InputArea a11y | Human micro-UX |
| [#663](https://github.com/iberi22/xavier/pull/663) | Roadmap CRUD + empty state | PR1 panel graph |
| [#676](https://github.com/iberi22/xavier/pull/676) | Memory KG API list+view | Ola Graph · 01 |
| [#674](https://github.com/iberi22/xavier/pull/674) | `/code/graph/view` | Ola Graph · 02 |
| [#675](https://github.com/iberi22/xavier/pull/675) | Mount routes | Ola Graph · 03 |
| [#677](https://github.com/iberi22/xavier/pull/677) | EntityGraph SQLite snapshot | Ola Graph · 06 |
| [#679](https://github.com/iberi22/xavier/pull/679) | Multi-layer UI + adapters + path fix | Ola Graph · 04+05 + integrate |

## Closed / superseded Jules drafts

| PR | Reason |
|----|--------|
| #671 | Conflicted multi-layer attempt; integrated via #679 |
| #672 | Incomplete stub (wrong for docs issue #670) |
| #673 | Conflicted with #663 `roadmapGraph.ts`; adapters in #679 |

## Issues

| Issue | Status | Via |
|-------|--------|-----|
| #664 Memory API | **Closed** | #676 + #675 + #679 |
| #665 Code view | **Closed** | #674 + #675 |
| #666 Mount routes | **Closed** | #675 + path fix #679 |
| #667 Adapters | **Closed** | #679 |
| #668 Multi-layer UI | **Closed** | #679 |
| #669 Durability | **Closed** | #677 |
| #670 Docs EPIC | **Closed** | Code path done; optional follow-up for `docs/GRAPH_LAYERS.md` polish |

## Live API surface on main

```
GET/POST /panel/api/graph
GET /memory/graph/view
GET /memory/graph/entities
GET /memory/graph/entities/{entity_id}
GET /memory/graph/relations
GET /code/graph/view
```

Panel: ConfigModal → **Roadmap | Memory KG | Code**

## Residual / follow-ups

1. Docs polish: `docs/GRAPH_LAYERS.md` + CORTEX guide alignment (was #670 soft close)
2. CI jobs failing in ~2s with empty steps (infra) — not blocking merges with admin
3. Manual QA of three layers against live Xavier `:8006`
