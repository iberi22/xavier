# ADR-039 — Per-repo memory stores, on-demand and interconnected

| Field | Value |
|-------|-------|
| **ID** | ADR-039 |
| **Status** | Accepted |
| **Date** | 2026-10-15 |
| **Authors** | Xavier architecture contributors |
| **Related** | `docs/design/xav-repo-multidb-design.md`, `docs/features/repo-memory-instance.md`, ADR-038 |

## Context

The daemon serves a single global memory store (`data/vec-store.sqlite3`) with all
repos multiplexed on the `workspace_id` column, while the per-repo code graph is
already isolated. The design (`docs/design/xav-repo-multidb-design.md` §1-§2)
proposes one physical SQLite memory store per opted-in repo
(`<product>/.xavier/memory.sqlite3`), opened on demand through the existing
`ConnectionManager`, searched via server-side fan-out with rank-based merge, and
encrypted per record with a per-repo key. The verified constraints behind it are:

| ID | Constraint (one line) | Code path given by the design |
|----|----------------------|-------------------------------|
| C1 | `ConnectionManager` pools already serve memory/metrics/security/auth/`vec_store_*`/`conv_*` — `get_code_graph_db` bypasses them with its own cache. | `src/codebase/connection_manager.rs:247-252`, `code-graph/src/db/mod.rs:20-47`, `src/memory/sqlite_vec_store/mod.rs:109` |
| C2 | `xavier_context_save` checkpoints into `conversations_db` (`conv_<workspace>`), not into `VecSqliteMemoryStore`. | `src/server/mcp/tools_context.rs:132-149`, `src/codebase/connection_manager.rs:168-176` |
| C3 | `xavier repo pack` packs only `code_graph.db`; repo memory is never packed. | `src/cli/commands/repo_pack.rs:50,259-266` |
| C4 | `memory_store_path_for()` has no runtime caller, and for the daemon repo it resolves to a new `<data_dir>/memory.sqlite3`, not `vec-store.sqlite3`. | `src/memory/repo_memory_store_path.rs:135-137` |
| C5 | Monorepo products collapse to the same git-root store; only `project_id` differs and the per-product-root comment is false today. | `src/codebase/repo_identity.rs:157-159,175-177`, `src/memory/repo_memory_store_path.rs:157-165`, `src/codebase/repo_config.rs:650-696` |
| C6 | `timeline_events` is the audit chain, not graph rows; the production row count was not verified (DB never opened). | `src/storage/migrations.rs:276-290`, `src/memory/sqlite_vec_store/audit.rs:72-84` |
| C7 | `vec-store.sqlite3` measures 1,573,040,128 B (+ WAL); the CLAUDE.md size figure is stale. | `ls -la data/` |
| C8 | A third multi-DB mechanism, `MultiDbManager`, exists with an in-memory-only registry and no eviction. | `src/storage/multi_db.rs:17-26`, `src/cli/server.rs:1059-1063` |
| C9 | `MemoryMax` was raised to 4G per the incident note (unit file not readable in-session); the design budgets for the 1 GiB worst case. | `docs/research/2026-08-24-agent-scanner-dedup-incident.md:33` |

## Decision

1. **Extend `ConnectionManager`; no parallel manager.** A store per repo is another
   `VecSqliteMemoryStore` instance on another path, reusing `GlobalConnectionProvider`
   registration (`src/memory/sqlite_vec_store/mod.rs:61-64,108-109`). New `PoolProfile`
   `RepoStore` (`max_size=4`, `min_idle=0` lazy, `cache_size=-2000`, `mmap_size=0`,
   class `Evictable`); infrastructure ids and the global store stay `Pinned` on the
   unchanged `Default` profile. Fix the four manager defects first: blocking `pool.get()`
   under the global write lock, evictable global store, no idle sweep, oversized per-pool profile.
2. **On-demand lifecycle with a thin domain layer.** `RepoStoreRegistry` (in-memory only,
   not a pool manager) resolves `root -> path`, enforces opt-in
   (`.xavier/config.toml` `memory_store = "repo"`) plus the `XAVIER_REPO_STORE_ROOTS`
   allowlist, caches open `VecSqliteMemoryStore` objects, and dedups concurrent opens.
   Eviction is LRU among `Evictable` over `XAVIER_MAX_REPO_STORES` (default 6) plus a
   60 s idle sweep (`XAVIER_REPO_STORE_IDLE_SECS`, default 300); close checkpoints via
   non-blocking `try_get()` outside the map lock. Store root is the nearest config
   ancestor (fixing C5), so each monorepo product owns its `<product>/.xavier/memory.sqlite3`.
3. **Interconnection is server-side fan-out plus rank-based (RRF) merge.** One embedding
   computation, bounded concurrency (`Semaphore(4)`, 2 s per-store timeout, partial
   results with per-store status), single global vector list ordered by raw cosine
   similarity (new `HybridSearchResult.vector_similarity`) only when the embedding model
   matches, per-store rank-based lexical/KG legs, post-filter by the requester clearance
   ceiling, and `(store, memory_id)` dedup. The no-`repos` path keeps the `QmdMemory` route unchanged.
4. **Per-repo record key in life, `.enc` package only to commit.** Live rows keep
   `at_rest.rs` per-record encryption but with `HKDF(master, "xavier-repo-record-key-v1:<project_id>")`;
   the global store keeps the node key untouched. The live `.sqlite3` is never committed;
   only the `VACUUM INTO` snapshot encrypted as `memory.sqlite3.enc` is.
5. **No migration of existing rows.** The global store stays `Pinned`, remains the default
   write target without a valid repo, and participates federated only read-only
   (`include_legacy=true` default). Repo stores are born empty; splitting the ~22k legacy
   rows by heuristic is explicitly not attempted.

### Rejected alternatives

- **ATTACH:** per-connection state with a 10-DB cap in rusqlite `bundled`, no saving on brute-force vector scans, non-comparable per-index BM25 scores, and encrypted packages cannot be attached.
- **Unified view (`TEMP VIEW … UNION ALL`):** inherits all ATTACH limits and additionally breaks FTS5 `MATCH`, which requires one concrete FTS table.
- **Full-file encryption of the live `.sqlite3`:** `bundled` SQLite cannot operate on an encrypted file (no SQLCipher), and it would kill FTS, ATTACH and `VACUUM INTO` while leaving plaintext on disk or costing the file size in RAM.
- **Parallel connection manager for repo memory:** two LRU managers would compete for the same fds/RAM without seeing each other — the exact defect the separate code-graph cache already has.
- **Reusing `MultiDbManager`:** unbounded cache, volatile in-memory registry, `data_dir`-rooted paths instead of `<repo>/.xavier`, and no real `get_store` caller.

## Consequences

- **Positive:** physical per-repo isolation without touching any existing row; one fd/RAM
  budget for the whole process; federated search degrades gracefully per store; commit
  artefacts stay encrypted and portable in format (if not yet in key sharing).
- **Negative / cost:** T4-T5 touch the connection manager used by every production store
  (mitigated by byte-identical `Default` profile, `Pinned` infra ids, and real-process
  verification); the SQL `legacy` leg ranks differently from `QmdMemory` and is documented
  as such; BM25 stays path-only on encrypted rows (as in production today per ADR-038);
  federated reads are not a cross-store consistent snapshot.
- **What would invalidate this decision:** measured `min_idle`/fd numbers from T3 and RAM numbers from T5
  showing the `RepoStore` profile does not fit the cgroup budget, or a verified need for
  cross-store atomic writes (2PC), which this design deliberately excludes.
- **Follow-up (recorded per design §6):** converging `MultiDbManager` and the separate code-graph cache onto the same registry is a later change, not part of this decision.
- **Out of scope:** cross-machine sync and master-key sharing are not part of this
  decision; they belong to the mesh wave (issue #3195) behind their own key-sharing ADR.
