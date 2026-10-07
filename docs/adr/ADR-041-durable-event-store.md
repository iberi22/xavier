# ADR-041 — Durable event store in the core database (`crates/xavier-events`, `/v1/events`)

| Field | Value |
|-------|-------|
| **ID** | ADR-041 |
| **Title** | Durable event store in the core database (`crates/xavier-events`, `/v1/events`) |
| **Status** | Accepted with conditions (2026-10-07) |
| **Date** | 2026-10-07 |
| **Revision** | 3 (repair round 2 — split; supersedes rev 2 for this scope only) |
| **Authors** | Architecture sub-agent (draft); owner review pending |
| **Reviewed inputs** | `ADR-041-REVIEW-muse.md`, `ADR-041-REVIEW-codex.md`, `ADR-041-REVIEW-muse-r2.md` (ACCEPT), `ADR-041-REVIEW-codex-r2.md` (REJECT, R1–R8) |
| **Verified against** | `origin/main` = `e41ac96c7ac0eaef803d4fa85189869ec39913e5` (`git rev-parse` confirmed) |
| **Related** | ADR-019 (plugin boundary), ADR-033 (hexagonal crates + fallback), ADR-035 (trunk), ADR-036 (in-tree plugin precedent), ADR-017 (mesh ACL), ADR-043 (task tree — depends on this ADR) |

## Context

`origin/main` has **three** event facilities: `src/coordination/events.rs` (~140 lines, in-RAM `tokio::broadcast` + `WebhookDispatcher`, no envelope/namespace/sequence/dedup, lease variants carry raw tokens); `src/coordination/event_log.rs` (448 lines, durable SQLite mirror `bus_events`, `id INTEGER PRIMARY KEY AUTOINCREMENT`, `since_id`/`since_ts` replay, retention `XAVIER_EVENT_LOG_MAX_AGE_DAYS`=7 / `_MAX_ROWS`=200 000, token redaction in `describe()`; opened at `src/cli/server.rs:611`; `GET /xavier/events`; own file `<state_dir>/events/bus_events.sqlite3`; its subscriber uses `try_send` and appends asynchronously, so publication is **not** a durable acknowledgement); `src/server/events.rs` + `RealtimeEvent` (WebSocket). What is missing is **not durability — it is a contract**: namespace, authenticated producer identity, idempotency, dedup, bounded envelope, store epoch, filter-bound cursor, archive-then-prune continuity, cross-namespace isolation, an at-rest contract for new payloads, and the join to the task/memory an event produced.

Constraints in force: **ADR-019** — core membership needs all three tests (no sockets/subprocesses/C toolchain; required to open/read/write/migrate/replicate a local DB with `cargo build` only; failures are storage failures); core depends on nothing plugin-side; a **higher minor is accepted with unknown-field tolerance** and `deny_unknown_fields` is **forbidden on contract types** (`ADR-019:166`). **ADR-033** — `xavier-domain` pure types, `xavier-ports` **traits only**, adapters depend on ports, only the root wires them, `FallbackChain<P>` + a declared degraded mode per outbound port, Wave 33 (`ADR-033:86`); neither crate exists. **ADR-036 precedent** — `crates/xavier-pageindex` is independent (no `xavier` dependency) with thin late root glue, storing trees in **its own SQLite file (WAL)** so core migrations are untouched (`ADR-036:84–85`).

Baseline verified on `origin/main`: 11 explicit members + root (`Cargo.toml:48–60`); `crates/` = `xavier-core-logic`, `xavier-pageindex`, `xavier-wasm`, and `xavier-ports`/`FallbackChain<P>` absent (`git ls-tree`, `git grep` → 0 hits); outbound ports = embedding, health, schema-init, threat-detection (`src/ports/outbound/mod.rs`); two migration histories share `schema_migrations`, no checksum column, `LATEST_SCHEMA_VERSION` stale at 5, and `backfill_legacy` marks versions without DDL (`src/storage/migrations.rs:40–48`, `src/memory/sqlite_vec_store/schema_impl.rs:68–83`, `src/storage/mod.rs:89–96`); at-rest crypto = AES-256-GCM, 12-byte nonce, DEK wrapped by the node record key (`XRK1‖nonce12‖ct48`), AD = (space id, kind, record id), `xr1:` prefix, **no per-namespace event key** (`src/crypto/encryption.rs`, `src/memory/sqlite_vec_store/at_rest.rs:4–30,202–230`); input-security/redaction facilities exist (`src/ports/inbound/input_security_port.rs:9`, `src/ports/outbound/threat_detection_port.rs`).

## Competing options

| Option | Description | Expected cost / complexity |
|--------|-------------|----------------------------|
| **A** | **Contracted store in the core DB**; ingest/cursor/replay in `crates/xavier-events` behind feature `events`; thin root glue; `EventStorePort` in `crates/xavier-ports`. | Medium. One extension migration, one feature crate, one shim, one import. Default build unchanged. |
| **B** | Own SQLite file (ADR-036 precedent, extending `bus_events.sqlite3`). | Medium. Another file to back up/quota/secure; **cannot be transactional with memory writes**; join becomes cross-DB. |
| **C** | Event logic inside the root crate (`src/events/`). | Low now, high later: couples a fast-moving domain to the ~280k-line unit; HTTP needs sockets → violates ADR-019. |
| **D** | External bus as system of record; Xavier read-only client. | Low. Two copies, non-idempotent replay, no join, a second service to operate. |
| **E** | Status quo (RAM broadcast + `event_log` mirror + external bus). | Zero. Keeps every contract gap above. |

## Simulation (REQUIRED)

**Recorded decision: accept on deterministic gates instead of a simulation report** (option (b) of the template), per the ADR-036 precedent (`ADR-036:56–60`: its simulation was never run; acceptance rested on a measured E2E). Engine `periferia/swal-sim/adr_sim.py` (separate repo); the model does not exist yet. **Runs/seed: not run — waived, not pending.** Model `adr_041_event_store`, scenarios bull/base/bear/worst, 5000 runs, seed 42. **Honesty note:** the waiver is **weaker than ADR-035's standard** and needs explicit owner re-confirmation before status → `Accepted`; there is no measured E2E, and the replacement evidence is the executable oracle table below, not a modeled score. **Runnable inputs if preferred:** `python3 adr_sim.py --model adr_041_event_store --runs 5000 --seed 42 --outdir reports` → `periferia/swal-sim/reports/ADR-041-adr_041_event_store.md` (+ `.json`).

## Assumptions and anchors

| Parameter | Value used | Source |
|-----------|------------|--------|
| Durable event log already exists; core may own an append-only table | yes | `src/coordination/event_log.rs`; `src/cli/server.rs:611`; ADR-019 test 2 + `migrations` in the allowed core set |
| HTTP surface must live outside the root crate | required | ADR-019 membership (sockets ⇒ plugin); ADR-036 thin glue |
| `crates/xavier-ports` / `FallbackChain<P>` absent today | **NO** — created in PR 0 | `Cargo.toml`; `git grep` 0 hits |
| Retention / dedup grace / max clock skew / snapshot max age | 90 d / 7 d / 300 000 ms / 3600 s | **ASSUMPTION** (env-configurable; confirm at implementation) |
| Cutover quiet window / max divergence; archive dir; payloads may be untrusted | 900 s / 0 rows; `data_dir/events/archive/`; yes | **ASSUMPTION** measured before P2; `src/storage/backup/` precedent; **ASSUMPTION** (agents/tools produce free text) |

## Decision

Adopt **Option A**. Design-only; it activates nothing.

### 1. Scope, ownership and the ADR-019 justification

- **Core owns the event DDL and append transaction** (`src/storage/events_store.rs`, `src/storage/events_retention.rs`). Honest ADR-019 test-2 justification: the store must be **transactional with memory-write projections**, covered by the same backup/quota/retention as the memory it refers to, and part of the same migration history. Option B is rejected **because it cannot satisfy the atomicity requirement** — not because plugins may not own files.
- `crates/xavier-events` owns validation, sampling, scan/redaction orchestration, cursor construction/validation and replay orchestration; no SQL, no sockets, no subprocesses, no C toolchain; depends only on `crates/xavier-ports`. **Root** owns HTTP glue, the `SecurityPort` and `EventStorePort` implementations, fallback assembly and single late route registration.
- **Bounded ADR-033 staging amendment (explicit deviation).** PR 0 creates `crates/xavier-ports` as a **staging contract crate** holding (a) pure types, (b) trait signatures, (c) **I/O-free policy primitives** (`FallbackChain<P>`, HMAC cursor helpers). The rev-2 claim "schema and traits only" is **withdrawn**: `FallbackChain` is behavior and has no home in ADR-033's map (`ADR-033:26–29`). At Wave 33 the crate splits mechanically — types → `xavier-domain`, traits → `xavier-ports`, policy primitives → `xavier-domain` with re-exports — with **no semantic change**; the owner must confirm the amendment at Wave 33.
- **Security bridge (no root types inside the plugin).** `crates/xavier-ports` defines `SecurityPort { scan(&self, input: &[u8]) -> ScanVerdict; redact(&self, input: &[u8]) -> RedactionOutcome }` (traits only); the **root** implements it over `InputSecurityPort`, `ThreatDetectionPort` and the existing redaction helpers, so `crates/xavier-events` never imports root security types.

### 2. Sequence: ONE global monotonic sequence (R1)

Exactly **one** counter per store: `event_seq(store_id PRIMARY KEY, last_seq)`, allocated by `UPDATE event_seq SET last_seq = last_seq + 1 WHERE store_id = ?1 RETURNING last_seq` inside the append transaction. `seq` is monotonic for `(store_id, epoch)`, global across namespaces, **never reused after pruning**, **non-contiguous per namespace**. PK `PRIMARY KEY (store_id, epoch, seq)` — with a single counter there is **no possible collision**, the concrete defect R1 executed against rev 2's per-namespace allocation; namespace filtering is a query predicate, never an allocation scope. `event_retention_floor(store_id, epoch, namespace, through_seq, updated_at)` holds the **per-namespace** prune watermark; a cursor for namespace `N` expires when `cursor.last_seq + 1 < retention_floor(N)`, so pruning A never expires B. **Gaps carry no loss signal**: gaps arise only from pruning (SQLite never burns a number on rollback), sampling drops and rejections allocate nothing, and loss is signalled **only** by `retention_floor`. Archive names are namespace-safe: `events-<store_id>-<epoch>-<ns_slug>-<from>-<through>.sqlite3.zst` with `ns_slug = base32(SHA-256(namespace))[0..16]`, and the manifest carries the full namespace — removing rev 2's cross-namespace filename collision.

### 3. Event envelope (concrete, ADR-019-compatible)

```rust
pub struct EventEnvelope { pub protocol_version: String /* "xavier-events/1" */,
  pub schema_version: u16 /* 1 */, pub message_id: Uuid /* correlation only */,
  pub namespace: String /* swal/{app_id}/{instance_id} */,
  pub event_type: String /* ^[a-z][a-z0-9._-]{0,127}$ */,
  pub task_id: Option<String>, pub run_id: Option<String>,
  pub occurred_at: i64 /* unix ms, producer clock, validated */,
  pub payload: serde_json::Value /* canonical JSON, <= 64 KiB, depth <= 16 */,
  pub import: Option<ImportMetadata> }
pub struct ImportMetadata { pub source: String, pub source_id: String,
  pub source_revision: Option<u64>, pub imported_at: i64 }
pub struct StoredEvent { /* envelope minus payload, plus */ pub event_id: Uuid, pub seq: i64,
  pub received_at: i64, pub principal_id: String, pub producer_id: String,
  pub disposition: Disposition, pub sanitizer_version: u16, pub payload: serde_json::Value }
```

**Server-assigned, never client-supplied:** `event_id` (UUID v7), `seq`, `received_at`, `principal_id`, `producer_id`, `disposition`, `store_id`, `store_epoch`, `sanitizer_version`. **Compatibility (R5):** higher **minor** accepted with unknown-field tolerance; `deny_unknown_fields` forbidden on contract types (`ADR-019:166`); unknown **optional** fields ignored, unknown **required operations/states** → typed `UnsupportedOperation`/`VersionSkew`; unknown `event_type` accepted as custom, never lifecycle. Bounds: payload ≤ 64 KiB canonical JSON, depth ≤ 16, single string ≤ 16 KiB, `idempotency_key` ≤ 128 bytes non-empty; `occurred_at` must satisfy `received_at - max_past_ms ≤ occurred_at ≤ received_at + XAVIER_EVENTS_MAX_CLOCK_SKEW_MS`, else `422 InvalidEvent`.

### 4. Identity, fingerprint, idempotency and retention — ONE rule (R2)

- **Uniqueness:** `UNIQUE(namespace, producer_id, idempotency_key)`; `Idempotency-Key` is mandatory, absent → `422 InvalidEvent`. **Fingerprint** = SHA-256 over the canonical tuple of immutable semantic fields (`namespace, producer_id, idempotency_key, event_type, task_id, run_id, occurred_at`) plus the canonical **sanitized** payload; it **excludes** `message_id, event_id, received_at, principal_id, disposition, import.imported_at`. A same-key retry with a changed `event_type`, task/run link, occurrence time or payload is therefore a **content conflict** (`409 IdempotencyConflict{existing_seq, existing_event_id}`), not a dedup hit.
- **Sanitizer version is stored, not fingerprinted:** `sanitizer_version` is a column and the fingerprint is recomputable from the stored sanitized payload, so a sanitizer upgrade never reclassifies a stored event; new events use the current version.
- **Interactive dedup lifetime — the single rule.** `event_idempotency.expires_at = received_at + (retention_days + grace_days)·86_400_000`, `grace_days = 7`; a tombstone is pruned **only** when `expires_at ≤ now` **and** its referenced event row is already pruned. The rev-2 "tombstone outlives pruning by a full retention window" rule is **deleted**. After expiry — i.e. once compaction has pruned both the event and its tombstone — a replayed key is a new event (`Persisted{deduped:false}`).
- **Import identity is permanent and separate.** `event_import_ledger(source, source_id, source_revision, event_id, state, updated_at)` is **never pruned**, so re-running an import inserts zero rows **forever**, independent of the interactive tombstone lifetime; keys are `"import:{source}:{source_id}:{revision}"`. A source update → new `source_revision` → new event; a source **delete** → an appended `source_deleted` tombstone, **never** a row delete.
- **Age basis and exemptions:** `received_at` (server clock) is the **only** retention age basis and `occurred_at` never decides pruning; lifecycle/task/security events are never **sampled**, but they **are archived and pruned with the range** once past retention age and the archive keeps them permanently, so `retention_floor(namespace)` advances uniformly and no exempt row pins it. Sparse ranges are normal: the prune selects all rows of the namespace with `seq < through_seq` in one transaction and the manifest records the actual `row_count`; a gap is never reported as loss.

### 5. Cursor, acknowledgement, replay and live recovery (R3)

- **Cursor** `{ v:1, store_id, epoch, namespace, query_fp, last_seq(exclusive), issued_at }`, HMAC-SHA256 with `XAVIER_EVENTS_CURSOR_KEY`, base64url; `query_fp` = first 8 bytes of SHA-256 over the canonical filter tuple `(namespace, producer, event_type, task_id, run_id, disposition, order=asc)`. The server validates signature, `store_id`, `epoch`, `namespace`, `query_fp`; mismatch → `400 InvalidCursor`. A **bare `after_seq` integer is never accepted alone**: it is accepted only with the same filter fields and the server re-issues a signed token (it does not "prove" the binding; it is re-bound server-side).
- **Pagination is forward-only ascending by `seq`.** `order=desc` is a stateless "newest N" convenience with **no cursor continuation**. A page scans the **whole** bounded range `[last_seq+1, high_watermark]` up to `scan_budget` rows; if the budget is exhausted first, `next_cursor.last_seq` = last **scanned** seq and `has_more = true`; only a fully scanned range advances to `high_watermark`, and `high_watermark` is read in the same snapshot as the page.
- **Delivery is at-least-once; ack is after the destination transaction commits.** `destination = core_table`: destination effect + `consumer_delivery_log(consumer_id, namespace, producer_id, event_id)` + `consumer_offsets` commit in **one `BEGIN IMMEDIATE`** transaction, so a crash after apply and before ack redelivers, the dedup row is found, and **no duplicate effect** occurs. `destination = external`: only at-least-once **redelivery** is promised, the consumer MUST be idempotent on `(consumer_id, namespace, producer_id, event_id)`, and the "no duplicate inserts" promise is not claimed. Retry: bounded exponential backoff `min(2^attempt·100 ms, 30 s)`, `max_attempts = 10`, then `ReplayStalled` (admin-visible) — never a silent drop; offsets are bound to `(consumer_id, store_id, epoch, namespace, query_fp)`.
- **Live recovery closes commit-before-notify.** `subscribe(cursor)` registers the live subscription, performs durable catch-up to `high_watermark`, then switches to live; independently a **periodic durable catch-up poll** every `XAVIER_EVENTS_POLL_INTERVAL_MS` (default 1000) re-reads `seq > last_delivered` for the bound filter, recovering a commit whose live notification never arrived. Receiver lag emits `StreamGap{from_seq,to_seq}` requiring a tail refill; `Last-Event-ID` carries the cursor on reconnect.
- **Volatile fallback never issues durable cursors.** Durable publish failure is `503 StoreUnavailable`; the volatile channel is explicit opt-in (`?volatile=1`) with `durable:false` on every message and no cursor.

### 6. Retention, archive and compaction with continuity (R3)

`archive_and_prune(admin, policy, through_seq) -> RetentionReport` is a resumable state machine `select → write_archive → fsync_file → fsync_dir → verify_checksum → mark_verified → prune → compact → report`, idempotent by `archive_id`, never on the request path. **Durability barrier:** `File::sync_all()` on the archive file **and** `fsync` on the containing directory happen **before** the manifest is marked `verified`; the manifest update and the prune commit in the same transaction, so a crash before `verified` leaves `state=writing` and deletes nothing, while a crash after `verified` but before `prune` resumes at `prune`. Prune deletes **exactly** the range covered by a `state=verified` manifest, in one transaction, and advances `event_retention_floor`; compaction is `VACUUM` only — never delete-without-archive. Continuity: (a) archives replay to identical envelopes (`POST /v1/events/replay --archive <id>`); (b) `410 CursorExpired{retention_floor, archives[], recovery}` lists covering archives; (c) a restored store gets a **new epoch** and old cursors are `InvalidCursor`; (d) a failed verify leaves `state=writing` and a resumable retry. Archive rows carry the same `xe1:` ciphertext as the live table (§9); the manifest is not secret.

### 7. DDL (core extension history)

```sql
CREATE TABLE event_store_meta (store_id TEXT PRIMARY KEY, epoch INTEGER NOT NULL, created_at INTEGER NOT NULL);
CREATE TABLE event_seq (store_id TEXT PRIMARY KEY, last_seq INTEGER NOT NULL DEFAULT 0);
CREATE TABLE events (store_id TEXT NOT NULL, epoch INTEGER NOT NULL, seq INTEGER NOT NULL, event_id TEXT NOT NULL,
  message_id TEXT NOT NULL, namespace TEXT NOT NULL, producer_id TEXT NOT NULL, principal_id TEXT NOT NULL,
  task_id TEXT, run_id TEXT, event_type TEXT NOT NULL, occurred_at INTEGER NOT NULL CHECK (occurred_at > 0),
  received_at INTEGER NOT NULL, payload_enc TEXT NOT NULL, payload_hash TEXT NOT NULL, sanitizer_version INTEGER NOT NULL,
  disposition TEXT NOT NULL CHECK (disposition IN ('accepted','quarantined')), quarantine_reason TEXT,
  import_source TEXT, import_source_id TEXT, import_source_revision INTEGER,
  PRIMARY KEY (store_id, epoch, seq), UNIQUE (store_id, epoch, event_id));
CREATE INDEX idx_events_ns_seq ON events(namespace, seq);   -- plus idx_events_ns_type_seq, idx_events_task
CREATE TABLE event_idempotency (namespace TEXT NOT NULL, producer_id TEXT NOT NULL, idem_key TEXT NOT NULL,
  fingerprint TEXT NOT NULL, event_id TEXT NOT NULL, seq INTEGER NOT NULL, store_id TEXT NOT NULL, epoch INTEGER NOT NULL,
  created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, PRIMARY KEY (namespace, producer_id, idem_key));
CREATE TABLE event_import_ledger (source TEXT NOT NULL, source_id TEXT NOT NULL, source_revision INTEGER NOT NULL,
  event_id TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('active','superseded','deleted')), updated_at INTEGER NOT NULL,
  PRIMARY KEY (source, source_id, source_revision));
CREATE TABLE event_retention_floor (store_id TEXT NOT NULL, epoch INTEGER NOT NULL, namespace TEXT NOT NULL,
  through_seq INTEGER NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY (store_id, epoch, namespace));
CREATE TABLE event_archive_manifest (archive_id TEXT PRIMARY KEY, store_id TEXT NOT NULL, epoch INTEGER NOT NULL,
  namespace TEXT NOT NULL, ns_slug TEXT NOT NULL, from_seq INTEGER NOT NULL, through_seq INTEGER NOT NULL,
  row_count INTEGER NOT NULL, sha256 TEXT NOT NULL, format_version INTEGER NOT NULL, path TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('writing','verified','pruned')), created_at INTEGER NOT NULL, completed_at INTEGER);
-- Also in the extension history: event_consumer_offsets(consumer_id, store_id, epoch, namespace, query_fp, last_delivered_seq,
-- updated_at, PK(consumer_id, store_id, epoch, namespace, query_fp)); consumer_delivery_log(consumer_id, namespace, producer_id,
-- event_id, delivered_at, PK(consumer_id, namespace, producer_id, event_id)); event_namespace_keys(namespace PK, wrapped_dek,
-- key_version, created_at); event_snapshots(snapshot_id PK, namespace, store_id, epoch, committed_watermark, snapshot_revision,
-- created_at, expires_at, payload_enc); audit_events(audit_id PK AUTOINCREMENT, principal_id, action, namespace, resource_id,
-- result, at); schema_migrations_ext(history, version, name, checksum, applied_at, PK(history, version)).
```

**Atomic write boundary:** `event_seq` increment + dedup/ledger reservation + event insert (+ optional projection row) in **one `BEGIN IMMEDIATE`** transaction. **Events are opaque by default**: an **opt-in** `event_projection` is rebuildable from `events` (never a source of truth), quarantine predicates apply to tail, stream, replay **and** projection, and the raw table is not FTS-indexed, so "BM25 for free" is **not** claimed. **Migration history:** `schema_migrations_ext` is a separate authoritative history applied by one idempotent `apply_extension_migrations(conn)` at the end of **both** open paths (`MigrationRunner::run`, `VecSqliteMemoryStore::init_schema_async`), including the legacy early-return path; SHA-256 mismatch fails **closed** (`MigrationChecksumMismatch`), `backfill_legacy` must never mark extension versions, and the stale `LATEST_SCHEMA_VERSION = 5` is corrected in the same PR. Fixtures: fresh, baseline-v6, legacy-v11, no-ledger, interrupted mid-apply, concurrent open.

### 8. Authorization, attribution, secrets (R6)

- **`AuthContext` is constructed only by the transport adapter** from the verified credential: `AuthContext { principal_id, credential_kind ∈ {HttpBearer, CliLocal, McpSession, Embedded, ImportDelegated}, namespace_grants, grant_epoch, issued_at, expires_at, correlation_id }`; no public constructor from client input, private fields with one `from_verified_credential(...)` in the root adapter. **Producer identity is server-derived:** `producer_id = "{credential_kind}:{principal_id}"`, and a client-supplied `producer_id` that differs → `403 NamespaceDenied`.
- **Import delegation (the explicit model).** `ImportDelegated` carries `{source, operator_principal, source_producer_map}`; with an `events.import(ns)` grant the **stored** `producer_id` is the preserved source producer (`"{source}:{source_producer}"`) while `principal_id` records the operator, and a client-supplied producer not covered by `source_producer_map` is rejected. This preserves historical producer identity *and* binds it to a credential without trusting the client.
- **Grants:** `publish(ns)`, `read(ns)`, `replay(ns)`, `events.import(ns)`, `admin.maintenance`. Revocation is by `grant_epoch`; **every** read path — tail, stream subscribe, each catch-up batch, each replay batch, snapshot reads — rechecks the current grant set, so revocation takes effect on a live stream within one poll interval. **Maintenance is admin-only** (`prune`, `vacuum`, bulk replay, snapshot creation, import), every invocation writes an `audit_events` row and **never** logs secrets, and **MCP is read-only by default** (publish requires `publish`).
- **Redactor/scanner failure rejects, never quarantines raw:** failure → `422 RedactionFailed{field, reason_code}` and **no payload is persisted**; a sanitized diagnostic row is stored only when the redactor itself can produce a token-free summary, otherwise only the reason code is stored.
- **Source conversion before broadcast:** `XavierEvent::{LeaseRenewed, LeaseBackoff, LeaseRevoked}` are converted at the publish boundary in `src/coordination/events.rs` (token → SHA-256 hash) **before** either broadcast or persistence, so no token-bearing payload reaches `bus_events`, the log, or the contracted store; a parity test asserts this. **Namespace isolation:** every row carries the namespace, cross-namespace reads need an explicit grant (ADR-017), and a cursor cannot span namespaces. **Codes:** 401 (no credential), 403 (grant/namespace denied), 409 (`IdempotencyConflict`), 410 (`CursorExpired`), 413 (`PayloadTooLarge`), 422 (`InvalidEvent`/`RedactionFailed`), 503 (`StoreUnavailable`).

### 9. Encryption at rest (R6)

On `origin/main`, `at_rest.rs` encrypts **memory-record** content/metadata with a per-record DEK wrapped by the node record key; it is **not** transparent encryption of a new events table and there is **no** per-namespace event key. This ADR defines one: ciphertext format `payload_enc = "xe1:" + base64url(nonce12 ‖ ct)`, AES-256-GCM with a **per-namespace event DEK** stored wrapped in `event_namespace_keys(namespace, wrapped_dek, key_version)` using the existing `XRK1‖nonce12‖ct48` wrap (`at_rest.rs:202–230`) and the node record key; **AAD** = `b"xavier-events/1|" ‖ store_id ‖ "|" ‖ namespace ‖ "|" ‖ event_id ‖ "|" ‖ seq`, so a ciphertext moved to another row/namespace/store fails authentication. The DEK is created lazily on first append for a namespace, and `payload_hash` is over the **plaintext** canonical sanitized payload (a hash is not secret), so dedup works without decryption. **Fail-closed:** if a node record key **is configured** but cannot be resolved or the unwrap fails, append returns `EventError::KeyUnavailable` and reads return the same — never plaintext fallback; if **no** record key is configured the store is `at_rest:"none"`, reported by `/health`. Archive rows carry the same `xe1:` ciphertext and the file is not re-encrypted. Decryption happens **inside** the core `EventStorePort` implementation; `xavier-events` and consumers see `StoredEvent` only after authorization, and projection reads decrypt through the same path (`tests/events_encryption.rs`).

### 10. Degraded capability states and per-port policies (R7)

```rust
pub enum CapabilityState { Healthy, Disabled, Unavailable, ReadOnlySnapshot }
pub struct CapabilityHealth { pub state: CapabilityState, pub reason: Option<String>,
  pub namespace: Option<String>, pub committed_watermark: Option<i64>,
  pub snapshot_revision: Option<u64>, pub snapshot_age_secs: Option<u64>,
  pub durable: bool, pub at_rest: &'static str }
```

**Snapshot owner/format:** the root adapter writes `event_snapshots` — an encrypted, namespace-scoped, immutable copy at a committed watermark/revision — with max age `XAVIER_EVENTS_SNAPSHOT_MAX_AGE_SECS` (default 3600); every snapshot read **rechecks** `grant_epoch` and namespace grants, and an expired or missing snapshot is `SnapshotUnavailable` (typed), never a silent empty page.

| Port | Primary | Fallback | Degraded mode | Terminal failure |
|------|---------|----------|---------------|------------------|
| `EventStorePort::append` | core SQLite | none | `Unavailable` → `503`; volatile opt-in only (`durable:false`, no cursor) | yes — publish fails |
| `EventStorePort::tail`/`subscribe` | core SQLite | authorized snapshot | `ReadOnlySnapshot` with stale metadata | yes — `503` |
| `SecurityPort` | input-security + threat-detection | none | `Unavailable` | yes — reject, never persist |
| `FallbackChain<P>` | first healthy adapter | ordered adapters | breaker: 5 consecutive failures → open 30 s → half-open probe | declared per port |

`Disabled` (feature off) is distinct from `Unavailable` (runtime fault); mutations fail typed on every degraded adapter; existing outbound ports keep ADR-033 Wave 33 policies (out of scope).

### 11. Cutover from the external bus (R7)

| Phase | Authority | Xavier role | Gate to advance |
|-------|-----------|-------------|-----------------|
| P0 dual-write | external bus | shim write-through + transactional outbox | source fingerprint captured; outbox drain = 0; divergence ≤ `XAVIER_CUTOVER_MAX_DIVERGENCE` (0 rows) across `XAVIER_CUTOVER_QUIET_SECS` (900) |
| P1 Xavier-read | external bus | Xavier serves tail/replay; external serves writes | consumer manifest 100 % migrated; `CursorExpired` recovery exercised |
| P2 Xavier-authoritative | **Xavier** | external bus read-only mirror; its writes rejected `410 BusRetired` | `XAVIER_CUTOVER_WATERMARK` reached; all batches acknowledged; rollback = flip back to P1 (external still holds history) |
| P3 retire | Xavier | external bus decommissioned | **owner acceptance** (production-data migration) |

**Schemas:** `cutover_source_checkpoint(source, checkpoint_kind, checkpoint_value, updated_at, PRIMARY KEY(source, checkpoint_kind))`; `cutover_outbox(outbox_id, source, payload_enc, created_at, drained_at)` written in the **same transaction** as the source-side write (transactional outbox); `cutover_manifest(source, revision, schema_fingerprint, row_count, max_source_id, captured_at)`; `cutover_ack(source, batch_id, batch_hash, last_source_id, acked_at)`. Checkpoints advance **only** for acknowledged durable batches, by atomic cursor-file replacement (temp + `fsync` + `rename` + directory `fsync`). **Source revisions/deletions:** update → new `source_revision` → new event; delete → appended `source_deleted` tombstone (append-only); a snapshot+delta (or a declared write freeze) covers updates/deletes to already-scanned rows. **Legacy in-tree bus migration:** existing `bus_events` rows are imported with `source="legacy-bus"`, `source_id=id`, `producer_id="legacy-bus:unknown"`, with token scrubbing applied **at import** (raw lease tokens may already be on disk); old cursors are mapped by `legacy_cursor_map(legacy_since_id, new_cursor, created_at)`; the legacy DB is retired only after every consumer reports the new cursor. Archive verification requires file **and** directory fsync before a durable manifest authorizes deletion (§6).

### 12. `crates/xavier-events` and `/v1/events`

Independent crate, depends only on `xavier-ports`; owns envelope validation, sampling (**before** persistence), scan/redaction orchestration through `SecurityPort`, cursor construction/validation, replay orchestration and DTOs; fail closed — scanner/redactor unavailable → no `accepted` row. HTTP surface (thin root glue, one late registration): `POST /v1/events`, `GET /v1/events?after_cursor=&limit=&namespace=&producer=&event_type=`, `GET /v1/events/stream`, `POST /v1/events/replay`, `POST /v1/events/prune`, `POST /v1/events/vacuum`; maintenance is admin-only. `EventStorePort` (object-safe, no root types): `append(auth, envelope, idempotency_key) -> Result<AppendReceipt, EventError>` with `Persisted{seq, event_id, deduped} | Quarantined{reason} | Dropped{reason}`; `tail(auth, query, cursor?, limit) -> Result<EventPage, EventError>`; `subscribe(auth, query, cursor) -> stream<Result<StreamItem, EventError>>`; page `{ events, next_cursor, high_watermark, retention_floor, has_more }`. Typed errors: `VersionSkew, NamespaceDenied, InvalidEvent, RedactionFailed, IdempotencyConflict, CursorExpired, InvalidCursor, PayloadTooLarge, UnsupportedOperation, KeyUnavailable, StoreUnavailable, SnapshotUnavailable, ReplayStalled`; auth and programming errors never surface as a service outage, and only `StoreUnavailable`/`SnapshotUnavailable`/`KeyUnavailable` trigger fallback.

## Consequences

- **Positive.** One contracted, namespace-scoped store replaces three ad-hoc facilities; replay is recoverable (cursor + dedup + archive); retention, backup, encryption and quota apply uniformly; the task↔event join becomes a query; the default build is unchanged (`events` off in `default`/`ci-safe`).
- **Negative / cost.** +2 workspace members here (`xavier-ports`, `xavier-events`; 11 explicit → 13, ADR-043 adds the third → 14); a stable versioned contract to maintain; write-amplification risk (sampling, quotas and retention are load-bearing); shim complexity during cutover.
- **What would invalidate this decision.** Measured crate-split cost exceeding the duplication removed; a compliance/multi-tenant requirement only the external bus can satisfy; retention/write amplification unworkable on target hardware after sampling; a security review finding the ingest path widens the attack surface more than the removed service; more than one breaking port-contract change per month over a quarter.

## Acceptance checks (deterministic; each mapped to a test or script)

All 21 rows are **future implementation gates**, not current acceptance evidence. Root integration tests run with `--no-default-features --features ci-safe,events`.

| # | Check | Command | Oracle | Negative control |
|---|-------|---------|--------|------------------|
| 1 | Envelope + append + dedup + tail | `cargo test -p xavier --no-default-features --features ci-safe,events --test events_api_contract` + `scripts/check-events-api.sh` | same key/same fingerprint → same `seq` + `deduped:true` across restart, concurrent retry, late retry; changed **event_type/task/run/occurred_at** → 409; two namespaces interleaved → globally distinct seq; oversized → 413; unknown optional tolerated, unknown required op → typed error | remove the `UNIQUE` reservation → must fail |
| 2 | Append-only structural proof | `... --test events_append_only` | bounded method inventory: no public API performs `UPDATE`/`DELETE`; retention is the only pruning path | direct SQL `UPDATE` succeeds (detector works) but is unreachable via the API |
| 3 | Replay ack + destination transaction | `... --test events_replay_delivery` | crash after destination apply, before ack → redelivery finds the dedup row, zero repeated effects; offsets bound to store/epoch/namespace/`query_fp`; external destination promises only at-least-once | drop the destination dedup write → must fail |
| 4 | Retention archive/prune durability | `... --test events_retention` | failpoints **before and after** file fsync and dir fsync; checksum verified; no unarchived delete; resumable; concurrent namespace archives; exact age predicate on `received_at`; exempt rows archived+pruned, floor advances | failpoint after prune before manifest commit → no partial state |
| 5 | Sampling determinism + quota | `... --test events_sampling_quota` | fixed clock/seed/budget → exact receipts; lifecycle/task/security never sampled; quota hit on an authoritative event → explicit `Dropped{reason}` | sample a lifecycle event → must fail |
| 6 | Global sequence + per-namespace floor | `... --test events_namespace_sequence` | two namespaces with interleaved seqs never collide; PK accepts both; `retention_floor(A)` does not expire namespace B; archive filenames differ for equal ranges | rev-2 per-namespace allocation DDL must fail this test |
| 7 | Fingerprint + tombstone + import ledger | `... --test events_idempotency_fingerprint` | same key + changed semantic field → 409; tombstone expiry → `deduped:false`; import ledger re-run inserts 0 rows **after** tombstone expiry; `occurred_at` skew → 422; sanitizer upgrade does not reclassify stored events | include `message_id` in the fingerprint → must fail |
| 8 | Stream recovery + revocation | `... --test events_stream_recovery` | dropped live notification → periodic poll delivers; `StreamGap` → tail refill; mid-stream `grant_epoch` revocation stops delivery within one poll; budget-exhausted filtered page does not jump to `high_watermark` | remove the poll → must fail |
| 9 | Encryption at rest | `... --test events_encryption` | `xe1:` format; AAD row/namespace/store mismatch fails auth; configured-but-unreadable key → `KeyUnavailable` on append **and** read, no plaintext; no record key → `at_rest:"none"`; archive rows decrypt | store plaintext → must fail |
| 10 | Authz matrix (all transports) | `... --test events_authz_matrix` | MCP/HTTP/CLI/embedded/import; forged producer → 403; import delegation preserves source producer + records operator; revoked grant fails before mutation; 401/403/409 | trust client `producer_id` → must fail |
| 11 | Quarantine across surfaces | `... --test events_quarantine` | exclusion across tail/stream/replay/projection; redactor failure rejects with no persisted payload; sanitized notification; token-bearing bus rows cannot bypass sanitization | quarantine only the retrieval arm → must fail |
| 12 | Admin-only maintenance + audit | `... --test events_admin_auth` | full grant matrix; audit identity verified; no secret logging; unauthorized prune/vacuum/bulk replay → no mutation | relax the admin check → must fail |
| 13 | Cursor expiry + archive replay | `... --test events_cursor_expiry` | prune below a cursor → 410 with covering archives; archive replay yields identical envelopes; restored store gets a new epoch; bare `after_seq` without matching filters → 400 | delete an archive before prune → must refuse |
| 14 | Restart durability | `... --test events_restart_durability` | barrier-controlled crash mid-publish: **acknowledged** receipts survive, in-flight unacknowledged requests retry and resolve; row count + cursor intact; tombstone survives | make append non-transactional → must fail |
| 15 | Event-history import | `... --test events_history_import` | fingerprint; stable IDs; namespace mapping; reject/quarantine; source update → new revision; source delete → tombstone; re-run after prune/expiry inserts 0 rows via the ledger | drop the source-id ledger → re-run duplicates |
| 16 | Projection rebuild + FTS boundary | `... --test events_projection_rebuild` | projection rebuildable from `events`; quarantine predicate honored; raw table not FTS-indexed | index raw events in FTS → must fail |
| 17 | Extension migration idempotency | `... --test storage_extension_migrations` | six fixtures; run twice → **zero new migration applications** (not "zero rows"); checksum mismatch fails closed; legacy backfill never marks extension versions | corrupt a checksum → open must fail closed |
| 18 | Degraded states (off vs fault) | `... --test capability_degraded` **and** `... --features ci-safe --test core_feature_off_build` | `Disabled` ≠ `Unavailable`; snapshot expiry/authorization/revocation; per-port policy table honored; mutations fail typed | inject DB failure with features **on** |
| 19 | Ledger + scoped gates | `scripts/verify-pipeline.sh` (scoped runner) | exit 0; declared targets **execute** on a clean fixture checkout; fail closed if a declared test is missing or skipped; secret scan + scoped fmt/clippy recorded | delete a declared test → pipeline must fail |
| 20 | Consumer cutover manifest + thresholds | `scripts/check-consumers.sh` | versioned sanitized manifest with initial inventory; numeric divergence/quiet thresholds; phase/authority table; outbox crash retry; legacy `bus_events` + old-cursor mapping; rollback oracle | live quiet period is rollout evidence, **not** a unit test |
| 21 | `FallbackChain<P>` + security bridge | `cargo test -p xavier-ports` | ordered adapters, breaker thresholds, health; `SecurityPort` impl parity with the existing HTTP path; no silent semantic change | fall through on an auth error → must fail |

## Files to modify (first three PRs)

- **PR 0 — `crates/xavier-ports` (staging):** `src/{lib,types,events,cursor,fallback,security}.rs`, `AGENTS.md`, `tests/{fallback_chain.rs,security_bridge.rs}`, `Cargo.toml` (member), `.gitcore/features.json` (`planned`).
- **PR 1 — core event store + extension history:** `src/storage/{extension_migrations,events_store,events_retention,migrations,mod}.rs`, `src/memory/sqlite_vec_store/schema_impl.rs`, `Cargo.toml` (`events = ["dep:xavier-events"]`, off in `default`/`ci-safe`), `.env.example` (`XAVIER_EVENTS_{RETENTION_DAYS,DEDUP_GRACE_DAYS,CURSOR_KEY,ADMIN_TOKEN,MAX_CLOCK_SKEW_MS,POLL_INTERVAL_MS,SNAPSHOT_MAX_AGE_SECS,ARCHIVE_DIR}` + sampling budget), `tests/events_{restart_durability,retention,append_only,encryption,namespace_sequence,idempotency_fingerprint}.rs`, `tests/storage_extension_migrations.rs`.
- **PR 2 — `crates/xavier-events` + `/v1/events` glue + cutover:** `crates/xavier-events/src/{lib,ingest,replay,auth}.rs`, `src/adapters/inbound/events_api.rs`, `src/server/mod.rs`, `src/coordination/{events.rs,event_log.rs}` (token conversion + shim), `scripts/{check-events-api.sh,check-consumers.sh,migrate-events-to-xavier.py}`, `tests/events_{api_contract,replay_delivery,sampling_quota,admin_auth,quarantine,cursor_expiry,stream_recovery,history_import,projection_rebuild,authz_matrix}.rs`, `tests/{capability_degraded.rs,core_feature_off_build.rs}`, `docs/design/events/README.md`, `.gitcore/features.json`.

## Follow-up verification

- **Metrics:** duplicate storage bytes removed; replay duplicates per 1 000 replayed events (target 0 for `core_table`); events lost on restart (target 0); time-to-first-event after crash; table growth per day vs budget; `/health` degraded frequency; at-rest coverage ratio.
- **Gates:** `scripts/verify-pipeline.sh` executes this ADR's ledger entries; `cargo test -p xavier-ports`/`-p xavier-events` run in the affected-crate CI slice; root targets use `--no-default-features --features ci-safe,events`. **Review date:** 30 days after PR 2 merges, or when the external bus is decommissioned.
- **Acceptance record:** two independent panel verdicts (neither the author) tied to the reviewed revision, plus either the simulation report or the recorded deterministic-gates waiver, are appended before status → `Accepted`.

## Acceptance conditions
Accepted by the owner after a 3-round panel (one reviewer ACCEPT, one REJECT). The round-3 blockers become mandatory tests of the first implementation PR: `pagination_never_skips_events` (property test over concurrent appends), `rollback_never_loses_acknowledged_writes`, `import_is_idempotent_across_sanitizer_versions`, and an explicit authority for task/run attribution. The simulation requirement is waived following the ADR-036 precedent. ADR-043 (task tree) is deferred until this ADR is implemented.

## Revision log

Repair round 2 against `ADR-041-REVIEW-codex-r2.md` (REJECT) and `ADR-041-REVIEW-muse-r2.md` (ACCEPT). Rev-2 reviewed SHA-256: `f857218d7a6335bc16dd670668de667e68fd9542fa9f191a77fc40e4ddbb018a`. All facts re-verified against `origin/main` `e41ac96c7ac0eaef803d4fa85189869ec39913e5`. Scope is the **event store only**; task-tree blockers (R4 and the task halves of R5–R8) are mapped in `ADR-043-DRAFT-atlas-task-tree.md`. `L<n>` = line in this file.

| Codex-r2 blocker (this ADR's share) | Status | Fix |
|---|---|---|
| **R1** per-namespace seq collides with `PRIMARY KEY(store_id,epoch,seq)`; archive filename lacks namespace; check #1 oracle wrong | FIXED | §2 (L60) — ONE global counter per `(store_id, epoch)`, no collision possible; per-namespace `event_retention_floor`; `ns_slug` in the archive name; check #6 added (L204) and #1's oracle changed to globally-distinct seq (L199) |
| **R2** payload-only fingerprint; two conflicting tombstone lifetimes; unbounded "re-run inserts zero rows"; `occurred_at` as age basis | FIXED | §4 (L84) — fingerprint over immutable semantic fields excluding correlation/server fields; ONE lifetime `retention_days + grace_days` prunable only after its event; separate **permanent** `event_import_ledger`; `received_at` the only age basis; checks #7 (L205) and #15 (L213) |
| **R3** destination ack/transaction missing; no periodic catch-up; filtered scan can skip; no archive fsync | FIXED | §5 (L92) — destination-transaction dedup (`consumer_delivery_log` + `consumer_offsets` in one txn), external = at-least-once only, retry/backoff/`ReplayStalled`, offsets bound to store/epoch/filter; periodic durable catch-up poll; `next_cursor` advances only past a fully scanned range; §6 (L100) file+dir fsync before `verified`; checks #3 (L201), #4, #8 |
| **R4** task forest/completion/atomic repo | **OUT OF SCOPE — moved** | `ADR-043-DRAFT-atlas-task-tree.md` §3–§6 |
| **R5** unknown-field rejection contradicts ADR-019; "schema and traits only" false; no security bridge; core task DDL unjustified | FIXED | §3 (L64) — higher-minor tolerance, `deny_unknown_fields` forbidden, typed errors for unsupported required ops; §1 (L53) — honest bounded ADR-033 staging amendment withdrawing "schema and traits only"; `SecurityPort` in ports + root impl so `events` never imports root security; §1 justifies core event DDL (atomicity with memory writes) and names Option B's rejection. Task-DDL half → ADR-043 §1 |
| **R6** `AuthContext` construction, run/task attribution, import identity, revocation on streams, secret handling, event/archive encryption | FIXED | §8 (L139) — single `from_verified_credential`, `ImportDelegated` with `source_producer_map`, `grant_epoch` rechecked on every read path, redactor failure **rejects** with no persisted payload, source token conversion before broadcast; §9 (L147) — `xe1:` format, per-namespace DEK + AAD, fail-closed `KeyUnavailable`, `at_rest:"none"` when unconfigured, archive ciphertext; checks #9, #10, #11 |
| **R7** cutover prose without tables/schemas/thresholds; legacy `bus_events`/cursor migration; archive durability; snapshot policy | FIXED | §11 (L172) — phase/authority/gate table with numeric divergence (0) and quiet window (900 s), checkpoint/outbox/manifest/ack schemas, transactional outbox, atomic cursor replacement, source update/delete transformations, legacy `bus_events` import + `legacy_cursor_map`; §10 (L151) — snapshot owner/format/max age/authorization, per-port policy table; §6 file+dir fsync; checks #4, #18, #20 |
| **R8** acceptance evidence not executable; #1 wrong package; #14 runner; #3/#16 no crash oracles; classify as future gates | FIXED | Acceptance table (L193) — #1 (L199) moved to the root target (`tests/events_api_contract.rs`), #19 names the scoped runner and fail-closed missing/skipped gate, #3 (L201) and #14 use barrier-controlled crash failpoints distinguishing acknowledged from in-flight writes; all 21 rows explicitly **future implementation gates**, not current evidence; the §Simulation waiver (L35) records the design-acceptance gap honestly |
| Muse M1–M10 (non-blocking) | RETAINED | Waiver record (§Simulation, L35), greenfield `TaskPort` (now ADR-043), member count (11 → 13 here, 14 with ADR-043), `FallbackChain` owner (PR 0), restart gate (#14), ascending cursor (§5), security reuse (§1/§8), named archive (§2/§6), replay semantics (§5), ADR-019/033 citation (§1) |

**Rejected with evidence:** none. Every blocker in this ADR's share of `ADR-041-REVIEW-codex-r2.md` was accepted and repaired; no reviewer finding is disputed on the facts. The one place this revision goes further than requested is withdrawing the false "schema and traits only" claim (§1).
