use anyhow::Result;
use crate::ports::outbound::schema_init::SchemaInitializer;
use crate::memory::sqlite_store::TABLE_MEMORIES;

use super::VecSqliteMemoryStore;

impl SchemaInitializer for VecSqliteMemoryStore {
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock();

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
        let has_primary_flag = Self::table_has_column(&conn, TABLE_MEMORIES, "primary_flag")?;
        if !has_primary_flag {
            conn.execute(&format!("ALTER TABLE {} ADD COLUMN primary_flag INTEGER DEFAULT 1", TABLE_MEMORIES), [])?;
        }
        let has_parent_id = Self::table_has_column(&conn, TABLE_MEMORIES, "parent_id")?;
        if !has_parent_id {
            conn.execute(&format!("ALTER TABLE {} ADD COLUMN parent_id TEXT", TABLE_MEMORIES), [])?;
        }

        // Add cluster_id, level, relation, revisions if missing
        if !Self::table_has_column(&conn, TABLE_MEMORIES, "cluster_id")? {
            conn.execute(&format!("ALTER TABLE {} ADD COLUMN cluster_id TEXT", TABLE_MEMORIES), [])?;
        }
        if !Self::table_has_column(&conn, TABLE_MEMORIES, "level")? {
            conn.execute(&format!("ALTER TABLE {} ADD COLUMN level TEXT DEFAULT 'atom'", TABLE_MEMORIES), [])?;
        }
        if !Self::table_has_column(&conn, TABLE_MEMORIES, "relation")? {
            conn.execute(&format!("ALTER TABLE {} ADD COLUMN relation TEXT", TABLE_MEMORIES), [])?;
        }
        if !Self::table_has_column(&conn, TABLE_MEMORIES, "revisions")? {
            conn.execute(&format!("ALTER TABLE {} ADD COLUMN revisions TEXT", TABLE_MEMORIES), [])?;
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
        if !Self::table_has_column(&conn, "relations", "weight")? {
            conn.execute("ALTER TABLE relations ADD COLUMN weight REAL DEFAULT 1.0", [])?;
        }
        if !Self::table_has_column(&conn, "relations", "contradicts_edge_id")? {
            conn.execute("ALTER TABLE relations ADD COLUMN contradicts_edge_id TEXT", [])?;
        }
        if !Self::table_has_column(&conn, "relations", "created_at")? {
            conn.execute("ALTER TABLE relations ADD COLUMN created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))", [])?;
        }
        if !Self::table_has_column(&conn, "relations", "updated_at")? {
            conn.execute("ALTER TABLE relations ADD COLUMN updated_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))", [])?;
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
        if !Self::table_has_column(&conn, "timeline_events", "summary")? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN summary TEXT", [])?;
        }
        if !Self::table_has_column(&conn, "timeline_events", "details")? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN details TEXT", [])?;
        }
        if !Self::table_has_column(&conn, "timeline_events", "agent_id")? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN agent_id TEXT", [])?;
        }
        if !Self::table_has_column(&conn, "timeline_events", "prev_hash")? {
            conn.execute("ALTER TABLE timeline_events ADD COLUMN prev_hash TEXT", [])?;
        }
        if !Self::table_has_column(&conn, "timeline_events", "curr_hash")? {
            // curr_hash was added at the same time as the table in some versions, but if missing:
            conn.execute("ALTER TABLE timeline_events ADD COLUMN curr_hash TEXT DEFAULT ''", [])?;
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

        // Vector search virtual tables
        Self::ensure_vector_index(&conn, self.config.embedding_dimensions)?;
        Self::ensure_fts_index(&conn)?;
        Self::ensure_timeline_sequence(&conn)?;

        Ok(())
    }
}
