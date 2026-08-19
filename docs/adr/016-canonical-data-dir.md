# ADR-016: Canonical Data Directory

**Status:** Accepted  
**Date:** 2026-08-17  
**Deciders:** SWAL Engineering  

## Context

Xavier has accumulated multiple `vec-store.sqlite3` files across different locations over time:

| Location | Records | Notes |
|----------|---------|-------|
| `apps/xavier/data/vec-store.sqlite3` | ~31.6K | **Canonical** — used by server |
| `apps/xavier/vec-store.sqlite3` | 809 | Legacy from early development |
| `apps/xavier/.xavier/vec-store.sqlite3` | 0 | Empty placeholder |
| `apps/shelf/vec-store.sqlite3` | 0 | Separate project (Shelf) |
| `~/proyectosSWAL/xavier/data/vec-store.sqlite3` | 0 | Old top-level path |
| `~/.local/share/xavier/vec-store.sqlite3` | 247 | XDG legacy |

This fragmentation causes:
- Confusion about which store is authoritative
- Data silently split across locations
- Backup scripts backing up the wrong files
- New developers (human or AI) writing to the wrong path

## Decision

**Single canonical data directory:** `apps/xavier/data/`

All persistent data files (vec-store, memory-store, auth DB, cache, etc.) MUST reside under `apps/xavier/data/`. The vec-store filename is `vec-store.sqlite3` within that directory.

### Consequences

**Positive:**
- Single source of truth for all vector data
- `VecSqliteStoreConfig::from_env()` always resolves to `data/vec-store.sqlite3` by default
- Startup guard warns if legacy files detected outside `data/`
- `scripts/consolidate-stores.py` provides safe migration path

**Negative:**
- Requires running consolidation script once for existing installations
- External tooling (backup scripts, monitoring) must be updated to use the canonical path

**Neutral:**
- `XAVIER_MEMORY_VEC_PATH` env var still allows override for testing/CI
- Shelf and other projects have their own independent data dirs (no conflict)

## Implementation

- **Startup guard:** Server emits a `WARN` alert if `vec-store*.sqlite3` files exist outside `data/` at boot time
- **Consolidation script:** `scripts/consolidate-stores.py` merges legacy stores into canonical with dedup (INSERT OR IGNORE on path+workspace_id)
- **ADR:** This document

## Related

- `VecSqliteStoreConfig::from_env()` in `src/memory/sqlite_vec_store/config.rs`
- `scripts/consolidate-stores.py` — migration tool
- ADR-006: Vector Store Local SQLite Vec
