use crate::memory::sqlite_store::TABLE_MEMORIES;
use crate::ports::outbound::schema_init::SchemaInitializer;
use anyhow::Result;
use libsql::{params, Connection};

use super::{vector, VecSqliteMemoryStore};

impl SchemaInitializer for VecSqliteMemoryStore {
    fn init_schema(&self) -> Result<()> {
        std::thread::scope(|s| {
            s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| {
                        anyhow::anyhow!("failed to build runtime for schema init: {}", e)
                    })?;
                rt.block_on(self.init_schema_async())
            })
            .join()
            .map_err(|_| anyhow::anyhow!("schema init thread panicked"))?
        })
    }
}

impl VecSqliteMemoryStore {
    pub(crate) async fn init_schema_async(&self) -> Result<()> {
        let conn = &self.conn;

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
        ))
        .await?;

        // Migration: Add primary_flag and parent_id columns if they don't exist
        let has_primary_flag =
            Self::table_has_column_async(conn, TABLE_MEMORIES, "primary_flag").await?;
        if !has_primary_flag {
            conn.execute(
                &format!(
                    "ALTER TABLE {} ADD COLUMN primary_flag INTEGER DEFAULT 1",
                    TABLE_MEMORIES
                ),
                (),
            )
            .await?;
        }
        let has_parent_id = Self::table_has_column_async(conn, TABLE_MEMORIES, "parent_id").await?;
        if !has_parent_id {
            conn.execute(
                &format!("ALTER TABLE {} ADD COLUMN parent_id TEXT", TABLE_MEMORIES),
                (),
            )
            .await?;
        }

        // Add cluster_id, level, relation, revisions if missing
        if !Self::table_has_column_async(conn, TABLE_MEMORIES, "cluster_id").await? {
            conn.execute(
                &format!("ALTER TABLE {} ADD COLUMN cluster_id TEXT", TABLE_MEMORIES),
                (),
            )
            .await?;
        }
        if !Self::table_has_column_async(conn, TABLE_MEMORIES, "level").await? {
            conn.execute(
                &format!(
                    "ALTER TABLE {} ADD COLUMN level TEXT DEFAULT 'atom'",
                    TABLE_MEMORIES
                ),
                (),
            )
            .await?;
        }
        if !Self::table_has_column_async(conn, TABLE_MEMORIES, "relation").await? {
            conn.execute(
                &format!("ALTER TABLE {} ADD COLUMN relation TEXT", TABLE_MEMORIES),
                (),
            )
            .await?;
        }
        if !Self::table_has_column_async(conn, TABLE_MEMORIES, "revisions").await? {
            conn.execute(
                &format!("ALTER TABLE {} ADD COLUMN revisions TEXT", TABLE_MEMORIES),
                (),
            )
            .await?;
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
        )
        .await?;

        // Migration: Add missing columns to relations table
        if !Self::table_has_column_async(conn, "relations", "weight").await? {
            conn.execute(
                "ALTER TABLE relations ADD COLUMN weight REAL DEFAULT 1.0",
                (),
            )
            .await?;
        }
        if !Self::table_has_column_async(conn, "relations", "contradicts_edge_id").await? {
            conn.execute(
                "ALTER TABLE relations ADD COLUMN contradicts_edge_id TEXT",
                (),
            )
            .await?;
        }
        if !Self::table_has_column_async(conn, "relations", "created_at").await? {
            conn.execute("ALTER TABLE relations ADD COLUMN created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))", ()).await?;
        }
        if !Self::table_has_column_async(conn, "relations", "updated_at").await? {
            conn.execute("ALTER TABLE relations ADD COLUMN updated_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))", ()).await?;
        }

        // Session tokens and auth
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_tokens (
                token TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                created_at DATETIME NOT NULL,
                expires_at DATETIME NOT NULL
            );",
        )
        .await?;

        // Tamper-evident hash chain (content chaining for integrity verification)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_chain (
                id TEXT PRIMARY KEY,
                prev_hash TEXT,
                content_hash TEXT NOT NULL,
                created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_memory_chain_created ON memory_chain(created_at);",
        )
        .await?;

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
        )
        .await?;

        // Migration: Add missing columns to timeline_events
        if !Self::table_has_column_async(conn, "timeline_events", "summary").await? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN summary TEXT", ())
                .await?;
        }
        if !Self::table_has_column_async(conn, "timeline_events", "details").await? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN details TEXT", ())
                .await?;
        }
        if !Self::table_has_column_async(conn, "timeline_events", "agent_id").await? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN agent_id TEXT", ())
                .await?;
        }
        if !Self::table_has_column_async(conn, "timeline_events", "prev_hash").await? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN prev_hash TEXT", ())
                .await?;
        }
        if !Self::table_has_column_async(conn, "timeline_events", "curr_hash").await? {
            conn.execute(
                "ALTER TABLE timeline_events ADD COLUMN curr_hash TEXT DEFAULT ''",
                (),
            )
            .await?;
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
        ).await?;

        // New native vector search table (Turso specific)
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS memory_embeddings (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                embedding F32_BLOB({})
            );",
            self.config.embedding_dimensions
        ))
        .await?;

        // Vector search indexes and timeline sequences
        Self::ensure_fts_index_async(conn).await?;
        Self::ensure_timeline_sequence_async(conn).await?;

        // Run automatic vector migration
        self.migrate_embeddings_on_startup(conn).await?;

        Ok(())
    }

    async fn migrate_embeddings_on_startup(&self, conn: &Connection) -> Result<()> {
        // 1. Check if we already migrated embeddings (meaning memory_embeddings is not empty)
        let count_stmt = conn
            .prepare("SELECT COUNT(*) FROM memory_embeddings")
            .await?;
        let mut count_rows = count_stmt.query(()).await?;
        let current_count = if let Some(row) = count_rows.next().await? {
            row.get::<i64>(0).unwrap_or_default()
        } else {
            0
        };

        if current_count > 0 {
            return Ok(());
        }

        // 2. Query all existing memories with non-null embeddings
        let select_stmt = conn.prepare("SELECT id, workspace_id, embedding FROM memory_records WHERE embedding IS NOT NULL").await?;
        let mut select_rows = select_stmt.query(()).await?;

        let mut migrated = 0;
        // 3. Loop and migrate each embedding to the new native vector table
        while let Some(row) = select_rows.next().await? {
            let id: String = row.get(0).map_err(anyhow::Error::msg)?;
            let workspace_id: String = row.get(1).map_err(anyhow::Error::msg)?;
            let embedding_blob: Vec<u8> = row.get(2).map_err(anyhow::Error::msg)?;

            let floats = vector::deserialize_embedding(&embedding_blob);
            if floats.is_empty() {
                continue;
            }

            let native_vec_blob = vector::serialize_embedding(&floats);

            conn.execute(
                "INSERT OR REPLACE INTO memory_embeddings (id, workspace_id, embedding) VALUES (?1, ?2, ?3)",
                params![id, workspace_id, native_vec_blob]
            ).await?;
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

    async fn table_has_column_async(conn: &Connection, table: &str, column: &str) -> Result<bool> {
        let stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .await?;
        let mut rows = stmt.query(()).await?;
        while let Some(row) = rows.next().await? {
            let col_name: String = row.get(1).map_err(anyhow::Error::msg)?;
            if col_name == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn ensure_timeline_sequence_async(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS timeline_sequence (
                workspace_id TEXT PRIMARY KEY,
                last_sequence INTEGER NOT NULL DEFAULT 0
            );",
        )
        .await?;
        Ok(())
    }

    async fn ensure_fts_index_async(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                id UNINDEXED,
                path,
                content,
                code_tokens
            );",
        )
        .await?;
        Ok(())
    }
}
