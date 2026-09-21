# Memory & Concurrency Analysis — `crates/xavier-core-logic` and `src/storage`

Static analysis. Targets: `crates/xavier-core-logic/src/{bm25,rrf,scoring,snippet,token_counter,types}.rs`
and `src/storage/{mod,migrations,multi_db,pragma}.rs`, `src/storage/backup/wal_streamer.rs`, with
cross-references into the adjacent `src/codebase/connection_manager.rs`,
`src/memory/connection_provider.rs` and `src/memory/sqlite_vec_store/schema_impl.rs` (the storage
modules delegate connection/PRAGMA behaviour there, so several findings span the boundary).

Summary of conclusions:

- **No hard cyclic-wait deadlock** was found purely inside the two analysed directories.
  The genuine risks are **latent lock-ordering/blocking hazards** and **races**, listed in §4.
- The dominant **memory** costs are repeated `to_lowercase()`/`String`/`Vec` allocations in the
  BM25/RRF/snippet hot paths, plus a handful of unbounded growth points in the WAL streamer and
  a redundant `VACUUM` in the checkpoint path.

---

## 1. `BM25` — `src/../.. crates/xavier-core-logic/src/bm25.rs`

### Findings

1. **`query_term_nqi` lowercases every document for every query term** (`bm25.rs:58-67`):

   ```rust
   for term in &query_terms {
       let n_qi = documents
           .iter()
           .filter(|d| d.content.to_lowercase().contains(term)) // O(Q × D) lowercases!
           .count() as f32;
   ```

   Each `to_lowercase()` allocates a full temporary copy of the document. For `Q` query terms and
   `D` documents that is `Q × D` full-content allocations on top of the `D` already made in the
   first pass. This dominates both allocation churn and time.

2. **Per-token `String` allocation on every occurrence** (`bm25.rs:41-43`):

   ```rust
   for token in tokens {
       *term_freqs.entry(token.to_string()).or_insert(0) += 1;
   }
   ```

   `token.to_string()` allocates a `String` for *every* whitespace token, even repeated ones,
   although only unique tokens need to be materialised as map keys.

3. **Query-token vectors double-allocate** (`bm25.rs:22-27`): the whole query is lowercased to one
   `String`, then each term is cloned into another `String` in `query_terms: Vec<String>`.

### Non-breaking optimisations

**1. Eliminate the `Q × D` lowercasing** by deriving `n_qi` from the term-frequency maps already
computed in the first pass (documents containing the term == maps containing the key). This is a
behavioural refinement (term match becomes token-scoped, which is the correct BM25 semantics; the
current substring `contains` also matches `"theory"` for `"the"`) and removes all extra allocations:

```rust
// after the first pass, instead of the contains() loop:
let mut query_term_nqi: std::collections::HashMap<&str, f32> =
    std::collections::HashMap::with_capacity(query_terms.len());
for term in &query_terms {
    let n_qi = doc_term_freqs
        .iter()
        .filter(|tf| tf.contains_key(*term))
        .count() as f32;
    query_term_nqi.insert(*term, n_qi);
}
```

**2. Allocate keys only for unique tokens** — `get_mut(&str)` does an allocation-free lookup before
falling back to insert:

```rust
for token in tokens {
    if let Some(c) = term_freqs.get_mut(token) {
        *c += 1;
    } else {
        term_freqs.insert(token.to_string(), 1);
    }
}
```

This removes `O(total tokens)` allocations and keeps `O(unique tokens)`.

**3. Borrow query terms instead of owning them** — keep the single lowercase copy, split into
`Vec<&str>` and drop the per-term `to_string()`:

```rust
let query_lower = query.to_lowercase();
let query_terms: Vec<&str> = query_lower.split_whitespace().collect();
```

(requires the `query_term_nqi` map and scoring loop to key on `&str`, which the code above already
does).

---

## 2. RRF — `crates/xavier-core-logic/src/rrf.rs`

### Findings

1. **The fusion key is re-cloned on every result even when the entry exists** (`rrf.rs:75`):

   ```rust
   scores
       .entry(result.path.clone())   // clones `path` for every item
       .and_modify(|entry| { ... })
       .or_insert_with(|| FusedScore::new(&result, contribution, weight));
   ```

   In the common deduplication case (same path appears across result sets) the `String` is cloned
   only to be thrown away.

2. **Content is copied with two overlapping "keep the best" policies** (`rrf.rs:36-47, 76-88`):
   `add_score` clones `content`/`source` when the original score is higher, and the `and_modify`
   block *also* clones `id`/`content`/`source`/`zone` when `updated_at` is newer. A path that wins
   on both criteria has its payload cloned twice.

3. `FusedScore` stores `content`/`source`/`id` as `String`s owned per entry; the fused output
   `Vec` then transfers ownership once. This is fine, but the interim clones in (2) are pure waste.

### Non-breaking optimisations

**1. Avoid the key clone with a `get_mut`/insert split** (allocation-free hot path for existing keys):

```rust
match scores.get_mut(&result.path) {
    Some(entry) => entry.add_score(&result, contribution, weight),
    None => {
        scores.insert(
            result.path.clone(),
            FusedScore::new(&result, contribution, weight),
        );
    }
}
```

**2. Consolidate the two policies into one canonical "is this a better representative?" test**, so
`content`/`source`/`id`/`zone` are replaced at most once per merge and the earlier clone in
`add_score` can go away:

```rust
fn add_score(&mut self, result: &ScoredResult, contribution: f32, weight: f32) {
    self.total_rrf += contribution;
    self.total_weight += weight;
    // keep the single best representative once, instead of `best_original_score` + `updated_at`
    // policies conflicting and double-cloning:
    if result.updated_at > self.updated_at {
        self.id = result.id.clone();
        self.content = result.content.clone();
        self.source = result.source.clone();
        self.zone = result.zone.clone();
        self.updated_at = result.updated_at;
        self.best_original_score = result.score;
    }
}
```

Note: unifying the criteria changes *which* representative survives ties/timestamp-vs-score
conflicts — pick the exact dominance rule you want, but pick **one** (the current behaviour can be
reproduced exactly by applying the `and_modify` block's `updated_at` rule alone and dropping the
`add_score` clones).

**3. Pre-size the map**: `HashMap::with_capacity(total_result_count)` if the caller can sum the
lengths of the input sets cheaply (the runner loop already knows them).

---

## 3. Scoring — `crates/xavier-core-logic/src/scoring.rs`

### Findings

1. **Per-document full lowercase copies** in all three scorers (`scoring.rs:34`, `:100-105`):
   `doc.content.to_lowercase()`, `session.summary.to_lowercase()`, `name`, `normalized_name`, and
   `description.to_lowercase()` allocate a copy per candidate. For a large result set this is
   `Σ content` bytes of transient memory per ranking pass.
2. **Substring counting** `content_lower.matches(term).count()` rescans the lowercase copy per term
   (`scoring.rs:40, 119`). Cheap per call but quadratic-ish in `query_terms × content`; combined
   with (1) it keeps the whole lowercase document alive during the term loop.

### Non-breaking optimisations

- Reuse the lowercased copy: it is already computed once per document — the current code is fine on
  that count. The measurable win is **not lowercasing when the query is already a prefix/substring
  match in the original case-insensitively for ASCII**, i.e. gate the `to_lowercase()` behind a
  cheap `eq_ignore_ascii_case`/`find` fast path (see §4 for the same helper), falling back to full
  Unicode folding only for non-ASCII. Identical results for ASCII text (code/docs — the common
  case), zero allocation for that path.
- `matches(term).count()` can early-exit once it reaches `MAX_TERM_OCCURRENCE_BONUS / TERM_OCCURRENCE_BONUS`
  occurrences (currently it always scans to the end even when the bonus is already capped). Behaviour-identical
  because the bonus is `.min(MAX_...)`.

---

## 4. Snippet — `crates/xavier-core-logic/src/snippet.rs`

### Findings

1. **`find_window` lowercases the entire body *and* the entire query into fresh `String`s** just to
   do one `.find()` (`snippet.rs:200-215`):

   ```rust
   let body_lower = body.to_lowercase();
   let q_lower = q.to_lowercase();
   if let Some(byte_idx) = body_lower.find(&q_lower) { ... }
   ```

   For a large document this is a `2× |body|` allocation per snippet call (and `extract` is called
   per search result). Combined with (2), the transient peak per call is several document copies.

2. **`extract` collects the whole body into `Vec<char>` twice** (`snippet.rs:48, 65`) and does
   index arithmetic with char counts. Each `Vec<char>` is 4 bytes/char, so a 1 MB document → ~4 MB
   per collection, twice, per call.

3. `clip_chars` correctly walks chars (no UTF-8 panic) at O(n) — fine.

4. *Adjacent correctness note (not memory):* `find_window` returns char indices measured against
   the **lowercased** copy, then `extract` uses those indices to slice the **original** body. For
   text whose case-folded form changes byte/char length (e.g. `İ` → `i̇`), the window can land off
   by a few chars. Keep this in mind if the snippet ever runs over non-ASCII-heavy corpora.

### Non-breaking optimisations

**1. Allocation-free ASCII fast path** in `find_window` — a byte-window, case-insensitive scan is
safe for an ASCII needle (needle bytes < 0x80 can never collide with UTF-8 continuation bytes
≥ 0x80, and ASCII bytes in the body are single bytes). Only fall back to the current
lowercase-and-`find` path for non-ASCII queries:

```rust
pub fn find_window(body: &str, query: &str) -> (usize, Option<usize>) {
    let q = query.trim();
    if q.is_empty() {
        return (0, None);
    }
    // Fast path: entirely-ASCII needle → byte-window scan, no allocation.
    let found = if q.is_ascii() {
        find_ascii_case_insensitive(body.as_bytes(), q.as_bytes())
    } else {
        let body_lower = body.to_lowercase();
        let q_lower = q.to_lowercase();
        body_lower.find(&q_lower)
    };
    let (start, len) = match found {
        Some(byte_idx) => (
            body[..byte_idx].chars().count(),
            q.chars().count(),
        ),
        None => return (0, None),
    };
    (start, Some(start + len))
}

fn find_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    'outer: for i in 0..=(haystack.len() - needle.len()) {
        for j in 0..needle.len() {
            if haystack[i + j].to_ascii_lowercase() != needle[j].to_ascii_lowercase() {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}
```

Identical results for ASCII queries (the overwhelming case for code/markdown); non-ASCII queries
take the existing allocation path unchanged.

**2. Replace the double `Vec<char>` collect + index math with `char_indices`** once, so no char
vector is materialised:

```rust
let start_char_idx_of = start;         // already a char index
let end_char_idx_of = end;             // already a char index
let snippet: String = body
    .char_indices()
    .filter(|(i, _)| *i >= start_idx && *i < end_idx)
    .map(|(_, c)| c)
    .collect();
```

`char_indices()` streams at O(n) with no 4-byte-per-char buffer.

---

## 5. Token counter — `crates/xavier-core-logic/src/token_counter.rs`

### Findings

1. **`collapse_repeated_ast_snippets` incubates a `Vec<String>` of every surviving line then
   `join("\n")`s it** (`token_counter.rs:57, 128`). Peak usage is roughly `2× |text|` (all lines
   owned *plus* the joined result) before the intermediate vector is dropped.
2. Duplicate detection is fine (window compares are borrowed slices — no allocation).

### Non-breaking optimisation

Build the result directly into a `String`, reserving capacity up front:

```rust
let mut compressed = String::with_capacity(text.len() + text.len() / 8);
for line in &result {
    compressed.push_str(line);
    compressed.push('\n');
}
if !text.ends_with('\n') {
    compressed.pop(); // remove the trailing '\n' added by the loop
}
```

Behaviour-identical output (join semantics + trailing-newline preservation), half the peak memory.

---

## 6. Types — `crates/xavier-core-logic/src/types.rs`

### Finding

`MemoryDocument::estimated_bytes` (`types.rs:200-233`) serialises metadata on every call:

```rust
+ self.metadata.to_string().len() as u64
```

This re-serialises the whole `serde_json::Value` to a temporary `String` *just* to measure it. If
this method feeds a size-accounting/eviction loop over many documents, each call is an
allocation-heavy O(|metadata|) re-encode, and it is inconsistent with the near-zero-cost field
measurements elsewhere in the method.

### Non-breaking optimisation

Measure the encoded form without materialising an owned string (a `serde_json::to_vec` still
allocates; the cheapest is a `serde_json::Serializer` counting wrapper, or simply cache the size
once). The lowest-risk change that preserves the public API and approximate result:

```rust
impl MemoryDocument {
    fn metadata_bytes(&self) -> u64 {
        // Cheap, allocation-free approximation: strings keys+values, numbers ~ 8 bytes.
        fn value_size(v: &serde_json::Value) -> u64 {
            match v {
                serde_json::Value::Null => 4,
                serde_json::Value::Bool(_) => 5,
                serde_json::Value::Number(_) => 8,
                serde_json::Value::String(s) => (s.len() + 2) as u64,
                serde_json::Value::Array(a) => 2 + a.iter().map(value_size).sum::<u64>(),
                serde_json::Value::Object(m) => {
                    2 + m
                        .iter()
                        .map(|(k, v)| (k.len() + 3) as u64 + value_size(v))
                        .sum::<u64>()
                }
            }
        }
        value_size(&self.metadata)
    }
}
```

Then use `self.metadata_bytes()` in `estimated_bytes`. If exact byte counts matter, pair this with
computing the size once at construction time instead of per call.

---

## 7. Storage — memory

### 7.1 `VACUUM` inside the startup checkpoint path — `src/storage/migrations.rs`

`checkpoint_dir` (`migrations.rs:77-96`) and `run` (`migrations.rs:103-111`) execute:

```rust
let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;");
```

`VACUUM` rebuilds the entire database: it is the single most expensive operation in SQLite for
**CPU, disk and transient memory** (`. VACUUM` on a 1 GB DB allocates the page cache + temporaries
and takes an exclusive lock for the whole duration). It is also reached here from a `Connection`
opened *without* `busy_timeout`, so on any DB with an open pooled connection it returns
`SQLITE_BUSY` immediately and is a silent no-op — meaning it is both dangerous (when it does run,
it stalls writers) and dead code (when it doesn't).

**Recommendation (non-breaking):** drop `VACUUM` from the automatic/hot path; keep only the
`wal_checkpoint(TRUNCATE)` (which is cheap and already safe in WAL mode). If a scheduled compaction
is genuinely desired, run it explicitly with a dedicated connection, `busy_timeout`, and a
cooldown/retry — never inside `MigrationRunner::run` or per-connection acquire.

Also note the whole body of `migrations::run` duplicates `maybe_wal_checkpoint` logic that
`apply_pragmas` already performs (`pragma.rs:21-35`) — see §8.1.

### 7.2 Unbounded manifest + full-file WAL reads — `src/storage/backup/wal_streamer.rs`

1. **`stream_wal_changes` allocates one buffer for the whole unread tail when
   `max_segment_size` is `None`** (`wal_streamer.rs:378-383`):

   ```rust
   let bytes_to_read = if let Some(max_sz) = self.config.max_segment_size {
       unread_bytes.min(max_sz)
   } else {
       unread_bytes   // can be tens of MB if a checkpoint was deferred
   };

   let mut buffer = vec![0u8; bytes_to_read];
   ```

   With the 50 MB checkpoint threshold used elsewhere, a single stream call can allocate a 50 MB+ `Vec`.
   **Recommendation:** give `with_max_segment_size` a sane default (e.g. 4 MB) — the
   `max_segment_size: Option<usize>` is only ever *more* granular, so this is behaviour-preserving
   (more, smaller segments) while removing the unbounded allocation.

2. **`BackupManifest` grows without bound** — `create_snapshot` and `stream_wal_changes` append
   forever and push everything into memory (`snapshots: Vec<SnapshotMetadata>` +
   `wal_segments: Vec<WalSegmentMetadata>`) *and* re-serialise the full manifest (pretty JSON) to
   disk on every segment (`save_to_file`, `wal_streamer.rs:157-165`). On a busy DB the manifest
   becomes the dominant memory and disk-IO consumer of the whole module.
   **Recommendations (non-breaking):**
   - Prune segments/snapshots older than a retention window (keep the newest snapshot and any
     segments after it). This is additive policy — defaults can keep today's behaviour if needed.
   - Write the manifest as compact (non-pretty) JSON and only when actually changed, optionally via
     an atomic temp-file + rename.

3. **`recover` slurps each segment with `fs::read`** (`wal_streamer.rs:568`) — transient memory is
   the largest segment size, unnecessary since it is immediately streamed into the target WAL file.
   **Recommendation:** copy with `std::io::copy` from a `File`, which adds no per-segment buffer.

4. **`create_snapshot` copies the live DB file directly** after a `TRUNCATE` checkpoint and opens
   two extra `Connection`s (`wal_streamer.rs:260-306`). The whole-DB `fs::copy` duplicates the file
   into the page cache and the checkpoint can stall behind active readers.
   **Recommendation (non-breaking):** use SQLite's online backup API
   (`rusqlite::backup::Backup`) which snapshots a consistent DB image without blocking writers and
   without the intermediate whole-file copy.

### 7.3 DB connection cache sizing & pragmas

`apply_pragmas` (`pragma.rs:21-35`) sets `cache_size=-8000` (8 MB) **per connection** and
`temp_store=MEMORY` (transient sorts/btree rebuilds live in the heap). Through
`ConnectionManager`, r2d2 pools are `max_size(10)` and up to `MAX_POOLS=16` (`connection_manager.rs:17, 182-188`),
so worst-case resident page cache is ~1.28 GB before temp-store spikes, and any big
`CREATE INDEX`/`VACUUM`/reindex uses RAM via `temp_store=MEMORY`.

**Recommendation (non-breaking, config-level):** add a `PRAGMA soft_heap_limit` (bounds temp-store
memory) and consider `PRAGMA cache_spill=ON` (default, but verify it has not been disabled), and
lower `MAX_POOLS`/pool size or make them configurable. These are additive pragmas that only cap
worst-case memory, so existing workloads are unaffected unless they were already near OOM.

### 7.4 Migrations are replayed on every store construction

`VecSqliteMemoryStore::new` → `init_schema_async` (`schema_impl.rs:66-90`) rebuilds an 11-entry
`MigrationManager` and calls `run_migrations` **every time a store is created** — including
`MultiDbManager::get_store` (`multi_db.rs:130-147`), which builds a brand-new store per call.

For an already-migrated DB, `run_migrations` still runs `MigrationRunner::new(vec![]).run(conn)`
which re-applies all PRAGMAs + performs a `wal_checkpoint(PASSIVE)` (`storage/mod.rs:213`), then
queries `current_version` and iterates the 11 legacy migrations. None of that is needed on the
second+ construction.

**Recommendations (non-breaking):**
- Fast-path `MigrationRunner::run`: if `current_version >= target_version`, return before touching
  pragmas (idempotency is preserved — nothing changes).
- In `MultiDbManager`, cache the opened store (`Arc<VecSqliteMemoryStore>`) and return a clone from
  `get_store` instead of re-running `new` (which reconnects and re-migrates) each call.

---

## 8. Storage — concurrency

### 8.0 Conclusion on deadlocks

No cyclic lock wait (A→B→A) exists that is *triggerable* purely within the analysed files, because
storage primitives never acquire two locks in opposite orders on the same path. The findings below
are the things that **can become hangs or errors under concurrency** and are worth hardening first.

### 8.1 `wal_checkpoint(TRUNCATE)` runs on every pool acquire

`PragmaCustomizer::on_acquire` (`connection_manager.rs:38-46`) calls `apply_pragmas`, which starts
with `execute_batch` of nine PRAGMAs and then `maybe_wal_checkpoint(conn, 50MB)`
(`pragma.rs:21-35`), which issues `PRAGMA wal_checkpoint(PASSIVE)` and, when triggered,
`PRAGMA wal_checkpoint(TRUNCATE)`.

- TRUNCATE waits for **every other connection's readers to finish** (bounded by
  `busy_timeout=5000`) — so a long-running read on *any* connection in the pool makes *every*
  subsequent acquire stall up to 5 s. This is the most likely user-visible "hang".
- Re-applying `journal_mode=WAL`, `cache_size`, `mmap_size`, `temp_store` on **every** acquire is
  redundant (they persist per connection) and adds lock/shared-cache traffic.

**Recommendation (non-breaking):** split the pragma application:
- one-time/heavy PRAGMAs at pool construction (already partly done via `initialize_wal_mode`), and
  only `busy_timeout` (+ optionally `foreign_keys`) per acquire;
- move `maybe_wal_checkpoint` out of the acquire path entirely — run it on the existing timer /
  startup path (`migrations::checkpoint`) or a background task.

### 8.2 Un-serialised `create_database` / `get_store` → concurrent migration runs

`MultiDbManager::create_database` validates the id, then performs the slow
`VecSqliteMemoryStore::new(store_config).await` and only afterwards inserts into
`databases` (`multi_db.rs:64-86`). Two concurrent `create_database(db_id)` (or a `create_database`
racing a `get_store`) both run `init_schema_async` against the **same new DB file** at the same
time. `MigrationRunner::run` records each version with a plain
`INSERT INTO schema_migrations (version, …)` (`storage/mod.rs:250-260`) — a second concurrent runner
re-inserting version N hits the `PRIMARY KEY(version)` and fails the migration with a confusing
"shouldn’t happen" user-visible error.

**Recommendations (non-breaking):**
- Record with `INSERT OR IGNORE INTO schema_migrations …` (defensive idempotency; safe for the
  single-runner path too).
- Serialise per-`db_id` initialisation in `MultiDbManager` (e.g. keep an in-flight set/map keyed by
  id, or insert a "creating" placeholder into `databases` *before* the slow init and remove it on
  error). This is the only place in the analysed dirs where the async `RwLock` state should also be
  the synchronisation point rather than just a cache.

### 8.3 `delete_database` holds the write lock across file I/O and a global disconnect

`delete_database` (`multi_db.rs:104-127`) takes `databases.write()`, removes the entry, and then —
while the guard is still live — calls `ConnectionManager::global().disconnect` (a `parking_lot`
write-lock on the global pool map) and `await`s `tokio::fs::remove_file` for the main DB + `-wal` +
`-shm`.

- Holding a `tokio::sync::RwLock` write guard across awaits is the classic recipe for
  "readers blocked on a slow writer" — `list_databases`/`get_database` stall behind file deletion.
- Lock ordering between `databases` (this module) and `ConnectionManager::pools` (global) is
  currently always `databases → pools`. Any future path that takes them in the reverse order
  creates a real deadlock; worth an explicit ordering comment.

**Recommendation (non-breaking):** shrink the critical section — remove the entry (and clone the
`WorkspaceDb`), drop the guard, then disconnect and delete files outside the lock:

```rust
let removed = { self.databases.write().await.remove(db_id) }; // guard dropped here
if let Some(ws) = removed { /* disconnect + remove files + -wal/-shm */ }
```

### 8.4 `WAL_INIT_LOCK` — a global `std::sync::Mutex` held while sleeping, on async worker threads

`ConnectionManager::connect_with_path` → `initialize_wal_mode` (`connection_manager.rs:46-77`)
takes the process-global `WAL_INIT_LOCK` mutex and, on lock contention, sleeps
`WAL_INIT_RETRY_DELAY` (50 ms) **inside the lock**, up to 10 times. This path executes
synchronously inside `new_with_provider` (`sqlite_vec_store/mod.rs:87-107`), i.e. on a Tokio worker
thread.

**Hazards:** it serialises *all* new-connection setup process-wide; on a current-thread
(`Runtime::new_current_thread`) executor — used elsewhere in this codebase for schema init
(`schema_impl.rs:29-43`) — a sleeping `std::thread::sleep` blocks the only worker, and any task that
needs that worker to complete the WAL init (e.g. the other side of a channel or a rendezvous) deadlocks.

**Recommendation (non-breaking):** wrap WAL initialisation in `tokio::task::spawn_blocking` (or run
the try-loop with `std::thread::yield_now`/a channel-based retry) so the executor worker is never
parked on a `std::sync::Mutex` + `sleep`. The current `thread::scope` + dedicated-runtime approach
in `schema_impl.rs` already shows the intended pattern for the migration path.

### 8.5 Benign but wasteful race — duplicate pool creation

`ConnectionManager::connect`/`connect_with_path` check `contains_key` under a read lock, then build
and insert under a write lock. Two concurrent connects for the same `project_id` can both build a
pool; the second `insert` overwrites the first, leaking the first pool's connections (open FDs +
8 MB page cache each) until GC closes them. Not a deadlock, but worth a `ManyEntry`/double-checked
insert to avoid spuriously holding 2× resources.

---

## 9. Prioritised recommendations (non-breaking)

| # | Area | Fix | Risk / impact |
|---|------|-----|---------------|
| 1 | `bm25` `n_qi` loop | derive `n_qi` from `doc_term_freqs`, no `Q×D` lowercasing | Low · removes dominant alloc churn |
| 2 | `bm25` term map | `get_mut` before `insert`; borrow query terms | Low · O(tokens)→O(unique) allocs |
| 3 | `connection_manager` acquire | split one-time vs per-acquire PRAGMAs; move WAL checkpoint off acquire | Low · kills the 5 s TRUNCATE stalls |
| 4 | `migrations::checkpoint_dir/run` | remove `VACUUM` from hot path | Low · removes OOM/stall + silent no-op |
| 5 | `multi_db` `delete_database` | drop write guard before I/O; comment lock order with `ConnectionManager` | Low · removes read-starvation |
| 6 | `wal_streamer` | default `max_segment_size`; stream segments with `io::copy`; prune manifest; use SQLite backup API | Low–Med · removes unbounded buffers/manifest growth |
| 7 | `MigrationRunner` | `INSERT OR IGNORE`; early-exit when up-to-date; cache store in `MultiDbManager` | Low · fixes concurrent-create race + per-call migration replay |
| 8 | `rrf` | `get_mut`/insert key split; single "best representative" policy; pre-sized map | Med · behavioural tie-break must be pinned in tests |
| 9 | `snippet` | ASCII fast-path `find_window`; `char_indices` instead of `Vec<char>` | Low–Med · validates unchanged for ASCII in tests |
| 10 | `token_counter` | build output `String` directly | Low · halves peak memory |
| 11 | `types::estimated_bytes` | allocation-free metadata sizing | Low · used in size/eviction loops |
| 12 | `WAL_INIT_LOCK` | `spawn_blocking` / non-blocking retry | Low–Med · avoids worker-parking deadlock on current-thread runtimes |

Items 1–7 and 9–12 are mechanical and safe to land independently with the existing tests as the
behavioural guard. Item 8 changes only *which* of several near-identical representatives survives a
merge — lock the exact tie-break rule into the RRF tests before changing it.

---

*Analysis only: no source files were modified in producing this report. Unless noted, all
recommendations are internal (private fields, private helpers, additive functions) and preserve the
existing public API of `xavier-core-logic` and `crate::storage`. A toolchain was not available in
the workspace, so these findings are structural rather than profiled.*
