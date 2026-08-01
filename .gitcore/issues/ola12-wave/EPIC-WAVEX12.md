# WAVEX 12 — Infrastructure, Quality, Mesh & DevOps

**Date:** 2026-07-31
**Tracks:** 4 (Infra, Code Quality, Mesh #115, DevOps)
**Total issues:** 14
**Execution:** Jules-dispatched (islands, ≤20 files/commit)

## Prerequisites (before dispatching)

- [ ] A1: Configure tmpfs at /build (16GB) + update .cargo/config.toml
- [ ] A2: Clean stale target/ dirs (~106GB recovery)
- [ ] A3: Fix 28 cargo warnings with cargo fix

## Track A — Infrastructure (A1-A3)

| # | Issue | Size | Files | Est. |
|---|-------|------|-------|------|
| A1 | tmpfs Rust builds | S | 2 | 30min |
| A2 | Disk cleanup stale targets | S | rm commands | 15min |
| A3 | Cargo warnings cleanup | M | ~15 | 1h |

## Track B — Code Quality (B1-B3)

| # | Issue | Size | Files | Est. |
|---|-------|------|-------|------|
| B1 | Dedup storage-inflation fix | M | 3 | 2h |
| B2 | Snippet prefix assertion fix | S | 2 | 1h |
| B3 | Reindex error propagation | S | 1 | 1h |

## Track C — Mesh #115 (C1-C5)

| # | Issue | Size | Files | Est. |
|---|-------|------|-------|------|
| C1 | Governance DAO on-chain stub | L | 8 | 4h |
| C2 | Data Commons pricing model | M | 5 | 3h |
| C3 | ACL role completion | M | 4 | 2h |
| C4 | libp2p peer discovery | L | 6 | 4h |
| C5 | Mesh health dashboard | S | 3 | 1.5h |

## Track D — DevOps & Packaging (D1-D3)

| # | Issue | Size | Files | Est. |
|---|-------|------|-------|------|
| D1 | Local CI pipeline | M | 3 | 2h |
| D2 | Panel UI build smoke | S | 2 | 1h |
| D3 | Release packaging docs | S | 2 | 1h |

## Execution Order

```
Phase 1 (local):  A1 → A2 → A3 (foundation)
Phase 2 (Jules):  B1, B2, B3, C3, C5, D2, D3 (parallel islands)
Phase 3 (Jules):  C1, C2, C4, D1 (depend on Phase 2)
```

## Guardrails

- ≤20 files per commit
- No TODO/FIXME in added lines
- No `jules` label until island harness passes
- GHA exhausted → local verify + manual merge after diff review
- Each issue self-contained (no cross-issue dependencies within phases)
