//! Database migrations for Xavier Unified Storage.
//!
//! ## Baseline migrations (v1–v5)
//!
//! These five migrations represent the *current* schema as of the introduction
//! of the struct-based [`MigrationRunner`](crate::storage::MigrationRunner).
//! They are *baseline* migrations: rather than reconstructing historical change
//! sets, they capture what `src/storage/` creates today, split into logically
//! ordered chunks so that a fresh database reaches the current schema version
//! (5) in five steps:
//!
//! - **v1** — core `memory_records` table + belief/session/checkpoint tables.
//! - **v2** — columnar (composite covering) indices on `memory_records`.
//! - **v3** — vector + full-text-search tables (`memory_embeddings`, `memory_fts`).
//! - **v4** — knowledge-graph entities/relations, integrity chain, timeline,
//!   panel UI, and other utility tables.
//! - **v5** — session token table recreation (with `id` PK) and recovery/auth
//!   tables (`users`, `backup_codes`).
//!
//! Legacy databases that predate this system (tables present, no
//! `schema_migrations`) are detected and backfilled to v5 by the runner without
//! re-running any DDL.

use anyhow::Result;
use rusqlite::Connection;

use crate::storage::{table_has_column, LegacyMigration, Migration, MigrationRunner};

// ---------------------------------------------------------------------------
// Struct-based baseline migrations (v1–v5).
// ---------------------------------------------------------------------------

/// All baseline migrations v1–v5, in version order.
///
/// Use with [`MigrationRunner::new`] (or `MigrationRunner::run` directly on a
/// fresh connection).
pub fn baseline_migrations() -> Vec<Migration> {
    vec![
        Migration::new(1, "initial_core_tables", V1_UP),
        Migration::new(2, "columnar_indices", V2_UP),
        Migration::new(3, "vector_and_fts", V3_UP),
        Migration::new(4, "graph_timeline_utils", V4_UP),
        Migration::new(5, "sessions_and_recovery", V5_UP),
    ]
}

/// Run the full baseline migration set on a connection. Convenience wrapper.
pub fn run(conn: &Connection) -> Result<()> {
    MigrationRunner::new(baseline_migrations()).run(conn)
}

// ===========================================================================
// v1 — Core memory + belief/session/checkpoint tables.
// ===========================================================================
const V1_UP: &str = r#"
-- Main memory records table.
CREATE TABLE IF NOT EXISTS memory_records (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    path TEXT NOT NULL,
    content TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    embedding BLOB,
    encrypted_dek BLOB,
    content_iv BLOB,
    metadata_iv BLOB,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    primary_flag INTEGER DEFAULT 1,
    parent_id TEXT,
    cluster_id TEXT,
    level TEXT DEFAULT 'atom',
    relation TEXT,
    revisions TEXT,
    embedding_status TEXT DEFAULT 'pending',
    embedding_attempts INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_memories_workspace ON memory_records (workspace_id);
CREATE INDEX IF NOT EXISTS idx_memories_path ON memory_records (workspace_id, path);
CREATE INDEX IF NOT EXISTS idx_memories_parent ON memory_records (parent_id);

-- Belief states.
CREATE TABLE IF NOT EXISTS belief_states (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    beliefs TEXT NOT NULL DEFAULT '[]',
    updated_at DATETIME NOT NULL
);

-- Session tokens (canonical form with id PK; v5 recreates legacy tables into this).
CREATE TABLE IF NOT EXISTS session_tokens (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    token TEXT NOT NULL,
    created_at DATETIME NOT NULL,
    expires_at DATETIME NOT NULL
);

-- Checkpoints.
CREATE TABLE IF NOT EXISTS checkpoint_records (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    name TEXT NOT NULL,
    data TEXT NOT NULL,
    created_at DATETIME NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_checkpoints_lookup ON checkpoint_records (workspace_id, task_id, name);
"#;

// ===========================================================================
// v2 — Columnar (composite covering) indices.
// ===========================================================================
const V2_UP: &str = r#"
-- Covering index for common workspace filtering with level + created_at ordering.
CREATE INDEX IF NOT EXISTS idx_memories_columnar_workspace_level
    ON memory_records (workspace_id, level, created_at, id);

-- Covering index for path-based lookups within a workspace.
CREATE INDEX IF NOT EXISTS idx_memories_columnar_path
    ON memory_records (workspace_id, path, updated_at);

-- Index for hierarchical navigation.
CREATE INDEX IF NOT EXISTS idx_memories_columnar_parent
    ON memory_records (workspace_id, parent_id, level);

-- Index for cluster analysis.
CREATE INDEX IF NOT EXISTS idx_memories_columnar_cluster
    ON memory_records (workspace_id, cluster_id, level);
"#;

// ===========================================================================
// v3 — Vector + full-text-search tables.
// ===========================================================================
const V3_UP: &str = r#"
-- Unified memory_embeddings table for sqlite-vec.
CREATE TABLE IF NOT EXISTS memory_embeddings (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    embedding BLOB NOT NULL
);

-- Unified memory_fts for full-text search.
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
    id UNINDEXED,
    path,
    content,
    code_tokens
);
"#;

// ===========================================================================
// v4 — Knowledge graph, integrity chain, timeline, panel UI, utilities.
// ===========================================================================
const V4_UP: &str = r#"
-- Knowledge-graph: entities & relations.
CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    properties TEXT,
    language_family TEXT,
    workspace_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_entities_workspace ON entities (workspace_id);
CREATE INDEX IF NOT EXISTS idx_entities_name ON entities (name);

CREATE TABLE IF NOT EXISTS relations (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    properties TEXT,
    weight REAL DEFAULT 1.0,
    confidence_score REAL DEFAULT 1.0,
    provenance_id TEXT,
    contradicts_edge_id TEXT,
    is_inferred INTEGER DEFAULT 0,
    source_language TEXT,
    target_language TEXT,
    workspace_id TEXT,
    created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
    updated_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
    FOREIGN KEY(source_id) REFERENCES entities(id),
    FOREIGN KEY(target_id) REFERENCES entities(id)
);
CREATE INDEX IF NOT EXISTS idx_relations_workspace ON relations (workspace_id);
CREATE INDEX IF NOT EXISTS idx_relations_source ON relations (source_id);
CREATE INDEX IF NOT EXISTS idx_relations_target ON relations (target_id);

CREATE TABLE IF NOT EXISTS memory_entities (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    relation_type TEXT,
    FOREIGN KEY(memory_id) REFERENCES memory_records(id),
    FOREIGN KEY(entity_id) REFERENCES entities(id)
);
CREATE INDEX IF NOT EXISTS idx_memory_entities_memory ON memory_entities(memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_entities_entity ON memory_entities(entity_id);

-- Integrity chain.
CREATE TABLE IF NOT EXISTS memory_chain (
    id TEXT PRIMARY KEY,
    workspace_id TEXT,
    prev_hash TEXT,
    content_hash TEXT NOT NULL,
    created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_memory_chain_created ON memory_chain(created_at);

-- Timeline events & sequence counter.
CREATE TABLE IF NOT EXISTS timeline_events (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    timestamp DATETIME NOT NULL,
    operation TEXT NOT NULL,
    summary TEXT,
    details TEXT,
    agent_id TEXT,
    prev_hash TEXT,
    curr_hash TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_timeline_workspace ON timeline_events(workspace_id);
CREATE INDEX IF NOT EXISTS idx_timeline_sequence ON timeline_events(sequence);

CREATE TABLE IF NOT EXISTS timeline_sequence (
    workspace_id TEXT PRIMARY KEY,
    last_sequence INTEGER NOT NULL DEFAULT 0
);

-- Panel UI & other utility tables.
CREATE TABLE IF NOT EXISTS panel_bookmarks (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS panel_widgets (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    type TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}',
    x INTEGER DEFAULT 0, y INTEGER DEFAULT 0,
    w INTEGER DEFAULT 1, h INTEGER DEFAULT 1,
    created_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS panel_graphs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    data TEXT NOT NULL DEFAULT '{}',
    created_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS encryption_metadata (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    salt BLOB NOT NULL,
    created_at DATETIME NOT NULL
);
CREATE TABLE IF NOT EXISTS notifications (
    id TEXT PRIMARY KEY,
    island_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0,
    severity TEXT NOT NULL
);
"#;

// ===========================================================================
// v5 — Session token table recreation + recovery/auth tables.
//
// session_tokens is already created in canonical form in v1; this migration
// adds the local-auth recovery tables (users, backup_codes).
// ===========================================================================
const V5_UP: &str = r#"
-- Local-auth recovery: users & backup codes.
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    recovery_seed_hash TEXT NOT NULL,
    two_factor_enabled INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);

CREATE TABLE IF NOT EXISTS backup_codes (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    code_hash TEXT NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(user_id) REFERENCES users(id)
);
CREATE INDEX IF NOT EXISTS idx_backup_codes_user ON backup_codes(user_id);
"#;

// ===========================================================================
// v7 — Embedding model metadata tracking.
// ===========================================================================
const V7_UP: &str = r#"
CREATE TABLE IF NOT EXISTS embedding_model_meta (
    key TEXT PRIMARY KEY,
    value TEXT
);
"#;

// ===========================================================================
// Legacy trait-based migration structs (kept for existing callers).
//
// These implement the old `Migration` trait (re-exported from
// `crate::storage`). They reproduce the original behaviour, including the
// idempotent ALTER/add-column repair logic that existed before the migration
// system was introduced.
// ===========================================================================

pub struct MigrationV1InitialSchema;

/// True if `table` exists in sqlite_master.
fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Idempotent column repair for pre-unified `entities` / `relations` tables.
///
/// Must run *before* V4_UP indexes that reference `workspace_id`, because
/// `CREATE TABLE IF NOT EXISTS` is a no-op on legacy tables missing those columns.
fn repair_entity_graph_columns(conn: &Connection) -> Result<()> {
    if table_exists(conn, "entities")? {
        if !table_has_column(conn, "entities", "language_family")? {
            conn.execute("ALTER TABLE entities ADD COLUMN language_family TEXT", [])?;
        }
        if !table_has_column(conn, "entities", "workspace_id")? {
            conn.execute("ALTER TABLE entities ADD COLUMN workspace_id TEXT", [])?;
        }
    }

    if table_exists(conn, "relations")? {
        let relation_columns = [
            ("weight", "REAL DEFAULT 1.0"),
            ("confidence_score", "REAL DEFAULT 1.0"),
            ("contradicts_edge_id", "TEXT"),
            ("is_inferred", "INTEGER DEFAULT 0"),
            ("source_language", "TEXT"),
            ("target_language", "TEXT"),
            ("workspace_id", "TEXT"),
            (
                "created_at",
                "DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))",
            ),
            (
                "updated_at",
                "DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))",
            ),
        ];
        for (col, def) in relation_columns {
            if !table_has_column(conn, "relations", col)? {
                conn.execute(
                    &format!("ALTER TABLE relations ADD COLUMN {} {}", col, def),
                    [],
                )?;
            }
        }
    }
    Ok(())
}

impl LegacyMigration for MigrationV1InitialSchema {
    fn version(&self) -> u32 {
        1
    }
    fn description(&self) -> &str {
        "Initial unified schema"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        // Apply the baseline v1 + v2 + v4 SQL via the struct-based constants so
        // the two representations can't drift. (Legacy callers historically
        // created the *entire* schema in one shot, so we replay v1+v2+v4 here.)
        conn.execute_batch(V1_UP)?;
        conn.execute_batch(V2_UP)?;
        conn.execute_batch(V3_UP)?;
        // Pre-repair legacy entity/relation tables so V4 indexes on workspace_id succeed.
        repair_entity_graph_columns(conn)?;
        conn.execute_batch(V4_UP)?;

        // Column repair for memory_records (idempotent ALTERs for legacy DBs).
        let memory_columns = [
            ("primary_flag", "INTEGER DEFAULT 1"),
            ("parent_id", "TEXT"),
            ("cluster_id", "TEXT"),
            ("level", "TEXT DEFAULT 'atom'"),
            ("relation", "TEXT"),
            ("revisions", "TEXT"),
            ("encrypted_dek", "BLOB"),
            ("content_iv", "BLOB"),
            ("metadata_iv", "BLOB"),
            ("embedding_status", "TEXT DEFAULT 'pending'"),
            ("embedding_attempts", "INTEGER DEFAULT 0"),
        ];
        for (col, def) in memory_columns {
            if !table_has_column(conn, "memory_records", col)? {
                conn.execute(
                    &format!("ALTER TABLE memory_records ADD COLUMN {} {}", col, def),
                    [],
                )?;
            }
        }

        // Ensure indexes exist after column repair (idempotent with V4_UP).
        if table_exists(conn, "entities")? && table_has_column(conn, "entities", "workspace_id")? {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_entities_workspace ON entities (workspace_id)",
                [],
            )?;
        }
        if table_exists(conn, "relations")? && table_has_column(conn, "relations", "source_id")? {
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_relations_source ON relations (source_id)",
                [],
            )?;
        }

        // Column repair for timeline_events.
        let timeline_columns = [
            ("summary", "TEXT"),
            ("details", "TEXT"),
            ("agent_id", "TEXT"),
            ("prev_hash", "TEXT"),
            ("curr_hash", "TEXT DEFAULT ''"),
        ];
        for (col, def) in timeline_columns {
            if table_exists(conn, "timeline_events")?
                && !table_has_column(conn, "timeline_events", col)?
            {
                conn.execute(
                    &format!("ALTER TABLE timeline_events ADD COLUMN {} {}", col, def),
                    [],
                )?;
            }
        }

        if table_exists(conn, "memory_chain")?
            && !table_has_column(conn, "memory_chain", "workspace_id")?
        {
            conn.execute("ALTER TABLE memory_chain ADD COLUMN workspace_id TEXT", [])?;
        }

        Ok(())
    }
}

pub struct MigrationV9EmbeddingStatus;

impl LegacyMigration for MigrationV9EmbeddingStatus {
    fn version(&self) -> u32 {
        9
    }
    fn description(&self) -> &str {
        "Add embedding_status and embedding_attempts columns to memory_records"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        if table_exists(conn, "memory_records")? {
            if !table_has_column(conn, "memory_records", "embedding_status")? {
                conn.execute("ALTER TABLE memory_records ADD COLUMN embedding_status TEXT DEFAULT 'pending'", [])?;
            }
            if !table_has_column(conn, "memory_records", "embedding_attempts")? {
                conn.execute("ALTER TABLE memory_records ADD COLUMN embedding_attempts INTEGER DEFAULT 0", [])?;
            }
        }
        Ok(())
    }
}

pub struct MigrationV8EntityGraphSnapshots;

impl LegacyMigration for MigrationV8EntityGraphSnapshots {
    fn version(&self) -> u32 {
        8
    }
    fn description(&self) -> &str {
        "Add entity_graph_snapshots table for knowledge graph persistence"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entity_graph_snapshots (
                workspace_id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }
}

pub struct MigrationV2ColumnarIndices;

impl LegacyMigration for MigrationV2ColumnarIndices {
    fn version(&self) -> u32 {
        2
    }
    fn description(&self) -> &str {
        "Implement columnar indices (composite covering indices)"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch(V2_UP)?;
        Ok(())
    }
}

pub struct MigrationV3UnifiedExtensions;

impl LegacyMigration for MigrationV3UnifiedExtensions {
    fn version(&self) -> u32 {
        3
    }
    fn description(&self) -> &str {
        "Unified Vector and FTS extensions"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch(V3_UP)?;
        Ok(())
    }
}

pub struct MigrationV4UnifiedIsolation;

impl LegacyMigration for MigrationV4UnifiedIsolation {
    fn version(&self) -> u32 {
        4
    }
    fn description(&self) -> &str {
        "Fix workspace isolation for graph data and event sequences"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        if !table_has_column(conn, "entities", "workspace_id")? {
            conn.execute("ALTER TABLE entities ADD COLUMN workspace_id TEXT", [])?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_entities_workspace ON entities (workspace_id)",
                [],
            )?;
        }
        if !table_has_column(conn, "relations", "workspace_id")? {
            conn.execute("ALTER TABLE relations ADD COLUMN workspace_id TEXT", [])?;
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_relations_workspace ON relations (workspace_id)",
                [],
            )?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS timeline_sequence (
                workspace_id TEXT PRIMARY KEY,
                last_sequence INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_relations_source ON relations (source_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_relations_target ON relations (target_id)",
            [],
        )?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_entities_name ON entities (name)",
            [],
        )?;
        Ok(())
    }
}

pub struct MigrationV5SessionTokensId;

impl LegacyMigration for MigrationV5SessionTokensId {
    fn version(&self) -> u32 {
        5
    }
    fn description(&self) -> &str {
        "Add id column to session_tokens"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        if !table_has_column(conn, "session_tokens", "id")? {
            // Recreate table with id column. Existing tokens are ephemeral.
            conn.execute_batch(
                "DROP TABLE IF EXISTS session_tokens;
                 CREATE TABLE session_tokens (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    token TEXT NOT NULL,
                    created_at DATETIME NOT NULL,
                    expires_at DATETIME NOT NULL
                 );",
            )?;
        }
        Ok(())
    }
}

pub struct MigrationV6RecoverySystem;

impl LegacyMigration for MigrationV6RecoverySystem {
    fn version(&self) -> u32 {
        6
    }
    fn description(&self) -> &str {
        "Add users and backup_codes tables for local auth recovery"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch(V5_UP)?;
        Ok(())
    }
}

pub struct MigrationV7EmbeddingModelMeta;

impl LegacyMigration for MigrationV7EmbeddingModelMeta {
    fn version(&self) -> u32 {
        7
    }
    fn description(&self) -> &str {
        "Add embedding_model_meta table for tracking active embedding model"
    }
    fn run(&self, conn: &Connection) -> Result<()> {
        conn.execute_batch(V7_UP)?;
        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Open an in-memory SQLite connection.
    fn mem_conn() -> Connection {
        Connection::open_in_memory().expect("failed to open in-memory db")
    }

    /// Assert the database reports the given schema version via MAX(version).
    fn assert_version(conn: &Connection, expected: u32) {
        let actual = MigrationRunner::current_version(conn).expect("version query failed");
        assert_eq!(
            actual, expected,
            "expected schema version {}, got {}",
            expected, actual
        );
    }

    #[test]
    fn fresh_db_migrates_to_latest_version() {
        let conn = mem_conn();
        run(&conn).expect("baseline migration failed");
        assert_version(&conn, 5);
        // The latest migration should have created the recovery tables.
        let users: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='users'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(users, 1, "users table should exist after v5");
    }

    #[test]
    fn rerunning_is_a_no_op() {
        let conn = mem_conn();
        run(&conn).expect("first run failed");
        let first = MigrationRunner::current_version(&conn).unwrap();

        // Second run must not error and must not duplicate migration records.
        run(&conn).expect("second run failed");
        let second = MigrationRunner::current_version(&conn).unwrap();
        assert_eq!(first, second, "version unchanged after re-run");

        let recorded: i64 = conn
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            recorded, 5,
            "exactly 5 migration rows after idempotent re-run"
        );
    }

    #[test]
    fn migrations_run_in_version_order() {
        // Register migrations out of order; runner should sort and apply ascending.
        let mut set = baseline_migrations();
        set.reverse(); // v5, v4, v3, v2, v1
        let conn = mem_conn();
        MigrationRunner::new(set).run(&conn).expect("run failed");

        // Read applied versions back in insertion order and confirm ascending.
        let mut stmt = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY rowid")
            .unwrap();
        let versions: Vec<i64> = stmt
            .query_map([], |r| r.get::<_, i64>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(versions, vec![1, 2, 3, 4, 5], "applied in ascending order");
    }

    #[test]
    fn legacy_db_is_detected_and_backfilled() {
        let conn = mem_conn();

        // Simulate a legacy DB: create the sentinel app table but no
        // schema_migrations bookkeeping table.
        conn.execute_batch(V1_UP).expect("seed legacy table");

        // Legacy detection should fire *before* the bookkeeping table exists.
        assert!(MigrationRunner::is_legacy_db(&conn).unwrap());

        // Running the baseline set should backfill to v5 without re-running DDL.
        run(&conn).expect("backfill failed");

        // All versions recorded with the legacy-backfill marker name.
        let mut stmt = conn
            .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
            .unwrap();
        let rows: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows.len(), 5);
        for (v, name) in &rows {
            assert_eq!(
                name, "legacy-backfill",
                "version {} should be marked legacy-backfill",
                v
            );
        }
        assert_version(&conn, 5);

        // Re-running must be a no-op (now that the bookkeeping table exists,
        // the DB is no longer "legacy").
        run(&conn).expect("second run failed");
        let recorded: i64 = conn
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            recorded, 5,
            "no duplicate rows after re-run on backfilled db"
        );
    }

    #[test]
    fn upgrades_schema_migrations_missing_name_column() {
        // Pre-name bookkeeping table (only version + applied_at) must not crash
        // when the runner records migrations that include the `name` column.
        let conn = mem_conn();
        // App tables present + bookkeeping without `name`, already at latest version.
        conn.execute_batch(V1_UP).expect("seed app tables");
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO schema_migrations (version) VALUES (1),(2),(3),(4),(5);",
        )
        .expect("seed old schema_migrations");

        run(&conn).expect("upgrade old schema_migrations");

        let has_name: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('schema_migrations') WHERE name='name'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            has_name, 1,
            "name column must exist after ensure_migration_table"
        );
        assert_version(&conn, 5);
    }

    #[test]
    fn migration_failure_rolls_back_transaction() {
        // A migration with deliberately broken SQL. It should fail and roll
        // back its transaction, leaving version 6 unrecorded.
        let migrations = vec![
            Migration::new(1, "ok", "CREATE TABLE IF NOT EXISTS t_good (x INTEGER);"),
            Migration::new(2, "bad", "THIS IS NOT VALID SQL;"),
        ];
        let conn = mem_conn();
        let err = MigrationRunner::new(migrations)
            .run(&conn)
            .expect_err("broken migration should have errored");
        assert!(
            err.to_string().contains("v2"),
            "error should reference failing migration v2, got: {}",
            err
        );

        // v1 should be recorded and committed; v2 should not.
        assert_version(&conn, 1);
        let recorded: i64 = conn
            .query_row("SELECT count(*) FROM schema_migrations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(recorded, 1, "only v1 should be recorded after rollback");
        // t_good created by committed v1 should still exist.
        let good: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='t_good'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(good, 1, "committed v1 table persists");
    }

    #[test]
    fn schema_migrations_records_applied_migrations() {
        let conn = mem_conn();
        run(&conn).expect("run failed");

        // Every baseline migration should be recorded with its canonical name.
        let mut stmt = conn
            .prepare("SELECT version, name, applied_at FROM schema_migrations ORDER BY version")
            .unwrap();
        let rows: Vec<(i64, String, String)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        let expected = [
            (1i64, "initial_core_tables"),
            (2, "columnar_indices"),
            (3, "vector_and_fts"),
            (4, "graph_timeline_utils"),
            (5, "sessions_and_recovery"),
        ];
        assert_eq!(rows.len(), expected.len(), "5 rows recorded");
        for ((v, name, ts), (ev, ename)) in rows.iter().zip(expected.iter()) {
            assert_eq!(v, ev, "version mismatch");
            assert_eq!(name, ename, "name mismatch for v{}", v);
            assert!(!ts.is_empty(), "applied_at populated for v{}", v);
        }
    }

    #[test]
    fn legacy_entities_missing_workspace_id_survives_v1_closure() {
        // Real Windows DBs predate workspace_id on entities. MigrationManager
        // re-runs V1 (which includes V4_UP indexes) — must not fail with
        // "no such column: workspace_id".
        let conn = mem_conn();
        // Start from baseline core tables (indexes need parent_id etc.), then
        // downgrade entities/relations to the pre-workspace_id shape seen in prod.
        conn.execute_batch(V1_UP).expect("seed core tables");
        conn.execute_batch(
            "DROP TABLE IF EXISTS entities;
             DROP TABLE IF EXISTS relations;
             CREATE TABLE entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                properties TEXT
             );
             CREATE TABLE relations (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                properties TEXT
             );",
        )
        .expect("downgrade entity graph to legacy shape");

        let mut manager = crate::storage::MigrationManager::new();
        manager.add_migration(MigrationV1InitialSchema);
        manager
            .run_migrations(&conn)
            .expect("legacy entities without workspace_id must upgrade");

        assert!(
            table_has_column(&conn, "entities", "workspace_id").unwrap(),
            "workspace_id added to entities"
        );
        let panel: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='panel_graphs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(panel, 1, "panel_graphs created by V4 replay");
    }
}
