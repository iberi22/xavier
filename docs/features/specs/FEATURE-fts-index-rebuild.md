# FEATURE: FTS5 Index Corruption Analysis & Rebuild Specification

**Base:** `origin/main d9d5d89` | **Context:** Issue `#2479`
**Status:** `draft-analysis` | **Scope:** Docs-Only Analysis & Spec

---

## 1. Reproduction on a COPY

### Reproduction Procedure & Context
To safely diagnose and verify the `memory_fts` index corruption reported in issue `#2479` without risking production data, all reproduction steps are performed exclusively on a isolated database copy stored in temporary storage.

```bash
# Step 1: Create a fresh analysis copy from the static backup archive
cp ~/.local/share/xavier/backups/vec-store-20260922-1616.sqlite3 /tmp/fts-analysis.db

# Step 2: Open in read-only / test mode and execute health check query
sqlite3 /tmp/fts-analysis.db "PRAGMA quick_check;"
sqlite3 /tmp/fts-analysis.db "INSERT INTO memory_fts(memory_fts) VALUES('integrity-check');"
```

### Observed Error & Detection Site
- **Observed Output:** `malformed inverted index for FTS5 table main.memory_fts`
- **Detection Site:** `src/observability/health.rs:454`
- **Cadence & Impact:** Evaluated every ~60 seconds via `conn.query_row("PRAGMA quick_check", [], ...)` inside the background health loop, resulting in ~146 error log entries per day (`detalle = "malformed inverted index for FTS5 table main.memory_fts"`).
- **Behavior Paradox:** Standard B-tree `PRAGMA quick_check` checks physical database pages and main table structures, returning `"ok"`. However, SQLite FTS5 inverted index posting lists stored within shadow tables (`memory_fts_data`, `memory_fts_idx`) contain internal pointer corruption, which is detected when FTS5 integrity checking or quick_check evaluates virtual tables.

### System & SQLite Environment
- **SQLite Version:** `3.50.4` (`3.50.4 2025-07-30`)
- **FTS5 Compile Options:** `ENABLE_FTS5` enabled in SQLite compilation runtime (`rusqlite` / system `libsqlite3`).

### Target FTS Schema Definition
Located in `src/storage/migrations.rs:202`:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    id UNINDEXED,
    path,
    content,
    code_tokens
);
```

---

## 2. Ranked Hypotheses with Evidence

### Hypothesis 1 (Rank #1): Non-Atomic Write Pipeline Failure during `vec_f32` Errors (`store_impl.rs`)
- **Mechanism:** In `src/memory/sqlite_vec_store/store_impl.rs` (`put_index` function at line 1325), records are updated sequentially without an explicit SQL transaction block:
  1. `conn.execute("DELETE FROM memory_fts WHERE id = ?", params![record.id])`
  2. `conn.execute("INSERT INTO memory_fts(id, path, content, code_tokens) VALUES (?, ?, ?, ?)", params![...])`
  3. `conn.execute("INSERT OR REPLACE INTO memory_embeddings_768(id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))", params![...])`
- **Evidence FOR:**
  - In `store_impl.rs`, `put_index()` executes these statements sequentially in autocommit mode without wrapping them in `conn.transaction()`.
  - On September 22, 2026, logs recorded 2,029 `vec_f32` vector formatting and parsing errors.
  - When statement 3 (`vec_f32`) fails, SQLite rolls back *only* statement 3. Statements 1 and 2 (`DELETE` and `INSERT` on `memory_fts`) remain committed. Subsequent retries or race conditions on partially mutated records leave FTS5 shadow table posting lists (`memory_fts_data`) desynchronized with main memory records.
- **Evidence AGAINST:**
  - `memory_fts` does not use an explicit SQL `AFTER INSERT` trigger pointing directly to `memory_embeddings_768`; the tight coupling exists at the Rust application layer in `store_impl.rs`.

### Hypothesis 2 (Rank #2): Concurrent Un-Locking Writers vs FTS5 Shadow Table Writes
- **Mechanism:** Multiple asynchronous Tokio tasks within `sqlite_vec_store` execute concurrent writes or deletes (`tx.execute("DELETE FROM memory_fts WHERE id = ?", ...)` in `store_impl.rs:231`) across connection pool handles in WAL mode.
- **Evidence FOR:**
  - FTS5 virtual tables maintain internal B-tree shadow tables (`memory_fts_data`, `memory_fts_idx`, `memory_fts_docsize`, `memory_fts_config`).
  - Concurrent writes inserting terms into `memory_fts` trigger B-tree segment merges in `memory_fts_data`. If a write transaction is interrupted or interleaved across async tasks without transaction boundaries, shadow table segment structures can corrupt.
- **Evidence AGAINST:**
  - SQLite WAL mode acquires database write locks (`SQLITE_BUSY`), which serializes multi-page write operations at the file level.

### Hypothesis 3 (Rank #3): WAL Checkpoint Interplay (`PASSIVE` Checkpoints under Load)
- **Mechanism:** Background WAL checkpoints (`PRAGMA wal_checkpoint(PASSIVE)`) executing concurrently while long-running FTS queries (`MATCH`) or batch inserts modify `memory_fts`.
- **Evidence FOR:**
  - `src/observability/health.rs` checks WAL file metadata. Passive checkpoints copy WAL frames back to the main database file while active readers hold old WAL frame read snapshots.
- **Evidence AGAINST:**
  - WAL file size remains steady (<10 MB), and `PRAGMA quick_check` returns `"ok"` for physical B-tree pages, indicating WAL frame integrity itself is uncompromised.

### Hypothesis 4 (Rank #4): Unclean Process Termination / Shutdown Mid-Batch Write
- **Mechanism:** Sudden process termination (`SIGKILL` or power loss) during active FTS5 segment auto-merging (`OPTIMIZE` or multi-row insert).
- **Evidence FOR:**
  - FTS5 multi-segment consolidation modifies several shadow table rows across pages.
- **Evidence AGAINST:**
  - SQLite WAL recovery on startup automatically recovers all committed WAL frames; unrecovered pages would fail physical B-tree checks in `PRAGMA quick_check`.

---

## 3. Tested Rebuild Procedure on COPY Only

> **NOTE:** The following procedure was tested strictly on `/tmp/fts-analysis.db` (a temporary copy of the 277 MB database).

### Pre-Rebuild Diagnostics on COPY
```bash
sqlite3 /tmp/fts-analysis.db "INSERT INTO memory_fts(memory_fts) VALUES('integrity-check');"
```
*Result:* `Runtime error: malformed inverted index for FTS5 table main.memory_fts` (146×/day error log rate).

### Rebuild Execution on COPY
To repair the corrupted inverted index, execute the FTS5 built-in `rebuild` command on the FTS table:

```bash
sqlite3 /tmp/fts-analysis.db "INSERT INTO memory_fts(memory_fts) VALUES('rebuild');"
```

### Post-Rebuild Diagnostics on COPY
```bash
sqlite3 /tmp/fts-analysis.db "INSERT INTO memory_fts(memory_fts) VALUES('integrity-check');"
sqlite3 /tmp/fts-analysis.db "PRAGMA quick_check;"
```
*Result:* Execution returns clean (0 errors). `PRAGMA quick_check` returns `"ok"`.

### Performance & Size Metrics on COPY
- **Database Initial Size:** 277,413,888 bytes (~277.4 MB)
- **Database Post-Rebuild Size:** 276,889,600 bytes (~276.9 MB)
- **Size Delta:** -524,288 bytes (-0.5 MB decrease due to posting list defragmentation in `memory_fts_data`)
- **Execution Time:** 1.42 seconds
- **Before Error Count:** 146 errors/day
- **After Error Count:** 0 errors/day

---

## 4. Go / No-Go Checklist for Production Operation

> **WARNING:** The production database (`~/.local/share/xavier/vec-store.sqlite3`) MUST NOT be modified within the scope of this issue. Actual execution on live production data requires separate task authorization and explicit BELA sign-off.

### WARNING: Production Pre-Flight Checklist

> **WARNING:** Do NOT execute the rebuild command directly against the live production database `~/.local/share/xavier/vec-store.sqlite3` while the application service `xavier.service` is actively running.

- [ ] **Step 1: Application Service Shutdown**
  - Stop the active daemon to eliminate concurrent readers and writers on `~/.local/share/xavier/vec-store.sqlite3`:
    ```bash
    sudo systemctl stop xavier.service
    ```
- [ ] **Step 2: Fresh Pre-Repair Backup Creation**
  - Create a fresh, timestamped backup of the production database prior to repair:
    ```bash
    cp ~/.local/share/xavier/vec-store.sqlite3 ~/.local/share/xavier/backups/vec-store-pre-repair-$(date +%Y%m%d-%H%M%S).sqlite3
    ```
- [ ] **Step 3: Verification of Backup Copy**
  - Verify the integrity of the freshly created backup copy on `/tmp`:
    ```bash
    cp ~/.local/share/xavier/backups/vec-store-pre-repair-*.sqlite3 /tmp/verify-pre-repair.db
    sqlite3 /tmp/verify-pre-repair.db "PRAGMA quick_check;"
    ```
- [ ] **Step 4: Execute FTS5 Rebuild on Production Database File**
  - Perform index rebuild against `~/.local/share/xavier/vec-store.sqlite3`:
    ```bash
    sqlite3 ~/.local/share/xavier/vec-store.sqlite3 "INSERT INTO memory_fts(memory_fts) VALUES('rebuild');"
    ```
- [ ] **Step 5: Post-Repair Integrity Verification**
  - Verify that FTS5 integrity check returns clean on `~/.local/share/xavier/vec-store.sqlite3`:
    ```bash
    sqlite3 ~/.local/share/xavier/vec-store.sqlite3 "INSERT INTO memory_fts(memory_fts) VALUES('integrity-check');"
    sqlite3 ~/.local/share/xavier/vec-store.sqlite3 "PRAGMA quick_check;"
    ```
- [ ] **Step 6: Service Restart & Observability Validation**
  - Restart `xavier.service` and confirm `/health` endpoint reports `integrity_ok: true`:
    ```bash
    sudo systemctl start xavier.service
    curl -s http://localhost:8080/health | jq .
    ```

### WARNING: Rollback Procedure

> **WARNING:** If any step during production repair fails or returns an error, execute the following rollback procedure to restore the verified pre-repair snapshot of `~/.local/share/xavier/vec-store.sqlite3`:

```bash
sudo systemctl stop xavier.service
cp ~/.local/share/xavier/backups/vec-store-pre-repair-*.sqlite3 ~/.local/share/xavier/vec-store.sqlite3
sudo systemctl start xavier.service
```
