---
name: xavier-maintenance-hygiene
title: Xavier Maintenance & Repository Hygiene Protocol
description: Canonical protocol for maintaining clean repository state, preventing directory sprawl, purging runtime database caches, and safely handling script/doc migrations.
tags:
  - xavier
  - hygiene
  - git-cleanup
  - cache
  - maintenance
category: maintenance
---

# Xavier Maintenance & Repository Hygiene Protocol

> **PURPOSE:** Prevent directory sprawl, quarantine temporary or obsolete scripts/databases, and keep Xavier lean and production-ready.

## 1. Directory Structure Boundaries
- **Learnings & Notes:** Always lowercase `.jules/`. Never create `.Jules/`.
- **Git Hooks:** Solely managed by `.husky/` (Husky 9). Never create `.githooks/`.
- **GitCore Ledger:** Centralized in `.gitcore/`.
- **Skills:** Project-level skills reside in `skills/<skill-name>/SKILL.md`.

## 2. Database & Cache Hygiene
Dynamic SQLite databases must **NEVER** be committed:
- Target runtime DBs: `data/*.sqlite3*`, `*.db*`, `.xavier/*.db*`, `metrics.db*`, `xavier_memory.db*`.
- Only static configuration fixtures (like `.xavier/maturity-anchors.json`) are tracked under `.xavier/`.

## 3. Scripts & Deprecation Policy
- **Obsolete Engines:** Cortex scripts (`scripts/*cortex*`) are officially legacy and isolated.
- **Platform Separation:** Prefer POSIX `.sh` and cross-platform Node/Python for CI/Linux workflows. Legacy `.ps1` scripts must not block Linux pipelines.
