use crate::memory::sqlite_store::TABLE_MEMORIES;
use crate::ports::outbound::schema_init::SchemaInitializer;
use anyhow::Result;
use rusqlite::{params, Connection};
use crate::codebase::connection_manager::ConnectionManager;

use super::{vector, VecSqliteMemoryStore};

impl SchemaInitializer for VecSqliteMemoryStore {
    fn init_schema(&self) -> Result<()> {
        let rt = match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle,
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
                runtime.handle().clone()
            }
        };
        rt.block_on(self.init_schema_async())
    }
}

impl VecSqliteMemoryStore {
    pub(crate) async fn init_schema_async(&self) -> Result<()> {
        let project_id = self.project_id.clone();
        let config = self.config.clone();

        ConnectionManager::global().with_conn(&project_id, move |conn| {
            // Create main memory table (same as SqliteMemoryStore)
            conn.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS {} (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    content TEXT NOT NULL,
                    metadata TEXT NOT NULL,
                    embedding BLOB,
                    created_at DATETIME NOT NULL,
                    updated_at DATETIME NOT NULL,
                    revision INTEGER NOT NULL,
                    primary_flag INTEGER DEFAULT 1,
                    parent_id TEXT,
                    cluster_id TEXT,
                    level TEXT DEFAULT 'atom',
                    relation TEXT,
                    revisions TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_memories_workspace ON {} (workspace_id);
                CREATE INDEX IF NOT EXISTS idx_memories_path ON {} (path);
                CREATE INDEX IF NOT EXISTS idx_memories_parent ON {} (parent_id);",
                TABLE_MEMORIES, TABLE_MEMORIES, TABLE_MEMORIES, TABLE_MEMORIES
            ))?;

            // Migration: Add primary_flag and parent_id columns if they don't exist
            if !Self::table_has_column(conn, TABLE_MEMORIES, "primary_flag")? {
                conn.execute(
                    &format!(
                        "ALTER TABLE {} ADD COLUMN primary_flag INTEGER DEFAULT 1",
                        TABLE_MEMORIES
                    ),
                    (),
                )?;
            }
            if !Self::table_has_column(conn, TABLE_MEMORIES, "parent_id")? {
                conn.execute(
                    &format!("ALTER TABLE {} ADD COLUMN parent_id TEXT", TABLE_MEMORIES),
                    (),
                )?;
            }

            // Add cluster_id, level, relation, revisions if missing
            if !Self::table_has_column(conn, TABLE_MEMORIES, "cluster_id")? {
                conn.execute(
                    &format!("ALTER TABLE {} ADD COLUMN cluster_id TEXT", TABLE_MEMORIES),
                    (),
                )?;
            }
            if !Self::table_has_column(conn, TABLE_MEMORIES, "level")? {
                conn.execute(
                    &format!(
                        "ALTER TABLE {} ADD COLUMN level TEXT DEFAULT 'atom'",
                        TABLE_MEMORIES
                    ),
                    (),
                )?;
            }
            if !Self::table_has_column(conn, TABLE_MEMORIES, "relation")? {
                conn.execute(
                    &format!("ALTER TABLE {} ADD COLUMN relation TEXT", TABLE_MEMORIES),
                    (),
                )?;
            }
            if !Self::table_has_column(conn, TABLE_MEMORIES, "revisions")? {
                conn.execute(
                    &format!("ALTER TABLE {} ADD COLUMN revisions TEXT", TABLE_MEMORIES),
                    (),
                )?;
            }

            // Knowledge graph for entity/relationship memory
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS entities (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    entity_type TEXT NOT NULL,
                    properties TEXT
                );
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
                    created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
                    updated_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now')),
                    FOREIGN KEY(source_id) REFERENCES entities(id),
                    FOREIGN KEY(target_id) REFERENCES entities(id)
                );
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
                CREATE INDEX IF NOT EXISTS idx_memory_entities_entity ON memory_entities(entity_id);",
            )?;

            // Migration: Add missing columns to relations table
            if !Self::table_has_column(conn, "relations", "weight")? {
                conn.execute(
                    "ALTER TABLE relations ADD COLUMN weight REAL DEFAULT 1.0",
                    (),
                )?;
            }
            if !Self::table_has_column(conn, "relations", "contradicts_edge_id")? {
                conn.execute(
                    "ALTER TABLE relations ADD COLUMN contradicts_edge_id TEXT",
                    (),
                )?;
            }
            if !Self::table_has_column(conn, "relations", "created_at")? {
                conn.execute("ALTER TABLE relations ADD COLUMN created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))", ())?;
            }
            if !Self::table_has_column(conn, "relations", "updated_at")? {
                conn.execute("ALTER TABLE relations ADD COLUMN updated_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))", ())?;
            }

            // Session tokens and auth
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS session_tokens (
                    token TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    created_at DATETIME NOT NULL,
                    expires_at DATETIME NOT NULL
                );",
            )?;

            // Tamper-evident hash chain (content chaining for integrity verification)
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS memory_chain (
                    id TEXT PRIMARY KEY,
                    prev_hash TEXT,
                    content_hash TEXT NOT NULL,
                    created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
                );
                CREATE INDEX IF NOT EXISTS idx_memory_chain_created ON memory_chain(created_at);",
            )?;

            // Audit timeline events
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS timeline_events (
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
                CREATE INDEX IF NOT EXISTS idx_timeline_sequence ON timeline_events(sequence);",
            )?;

            // Migration: Add missing columns to timeline_events
            if !Self::table_has_column(conn, "timeline_events", "summary")? {
                conn.execute("ALTER TABLE timeline_events ADD COLUMN summary TEXT", ())?;
            }
            if !Self::table_has_column(conn, "timeline_events", "details")? {
                conn.execute("ALTER TABLE timeline_events ADD COLUMN details TEXT", ())?;
            }
            if !Self::table_has_column(conn, "timeline_events", "agent_id")? {
                conn.execute("ALTER TABLE timeline_events ADD COLUMN agent_id TEXT", ())?;
            }
            if !Self::table_has_column(conn, "timeline_events", "prev_hash")? {
                conn.execute("ALTER TABLE timeline_events ADD COLUMN prev_hash TEXT", ())?;
            }
            if !Self::table_has_column(conn, "timeline_events", "curr_hash")? {
                conn.execute(
                    "ALTER TABLE timeline_events ADD COLUMN curr_hash TEXT DEFAULT ''",
                    (),
                )?;
            }

            // Checkpoints table
            conn.execute_batch(
                &format!(
                    "CREATE TABLE IF NOT EXISTS {} (
                        id TEXT PRIMARY KEY,
                        workspace_id TEXT NOT NULL,
                        task_id TEXT NOT NULL,
                        name TEXT NOT NULL,
                        data TEXT NOT NULL,
                        created_at DATETIME NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_checkpoints_lookup ON {} (workspace_id, task_id, name);",
                    crate::memory::sqlite_store::TABLE_CHECKPOINTS,
                    crate::memory::sqlite_store::TABLE_CHECKPOINTS
                )
            )?;

            // New native vector search table (Turso specific)
            conn.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS memory_embeddings (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    embedding F32_BLOB({})
                );",
                config.embedding_dimensions
            ))?;

            // Vector search indexes and timeline sequences
            Self::ensure_fts_index(conn)?;
            Self::ensure_timeline_sequence(conn)?;

            // Run automatic vector migration
            Self::migrate_embeddings_on_startup(conn)?;

            Ok(())
        }).await
    }

    fn migrate_embeddings_on_startup(conn: &Connection) -> Result<()> {
        // 1. Check if we already migrated embeddings (meaning memory_embeddings is not empty)
        let current_count: i64 = conn.query_row("SELECT COUNT(*) FROM memory_embeddings", (), |row| row.get(0))?;

        if current_count > 0 {
            return Ok(());
        }

        // 2. Query all existing memories with non-null embeddings
        let mut select_stmt = conn.prepare("SELECT id, workspace_id, embedding FROM memory_records WHERE embedding IS NOT NULL")?;
        let mut select_rows = select_stmt.query(())?;

        let mut migrated = 0;
        // 3. Loop and migrate each embedding to the new native vector table
        while let Some(row) = select_rows.next()? {
            let id: String = row.get(0)?;
            let workspace_id: String = row.get(1)?;
            let embedding_blob: Vec<u8> = row.get(2)?;

            let floats = vector::deserialize_embedding(&embedding_blob);
            if floats.is_empty() {
                continue;
            }

            let native_vec_blob = vector::serialize_embedding(&floats);

            conn.execute(
                "INSERT OR REPLACE INTO memory_embeddings (id, workspace_id, embedding) VALUES (?1, ?2, ?3)",
                params![id, workspace_id, native_vec_blob]
            )?;
            migrated += 1;
        }

        if migrated > 0 {
            tracing::info!(
                "Migración automática de libSQL completada: {} embeddings transferidos con éxito.",
                migrated
            );
        }

        Ok(())
    }

    fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
        let mut rows = stmt.query(())?;
        while let Some(row) = rows.next()? {
            let col_name: String = row.get(1)?;
            if col_name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ensure_timeline_sequence(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS timeline_sequence (
                workspace_id TEXT PRIMARY KEY,
                last_sequence INTEGER NOT NULL DEFAULT 0
            );",
        )?;
        Ok(())
    }

    fn ensure_fts_index(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                id UNINDEXED,
                path,
                content,
                code_tokens
            );",
        )?;
        Ok(())
    }
}
