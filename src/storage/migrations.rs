//! Database migrations for Xavier Unified Storage.

use anyhow::Result;
use rusqlite::Connection;
use crate::storage::{Migration, table_has_column};

pub struct MigrationV1InitialSchema;

impl Migration for MigrationV1InitialSchema {
    fn version(&self) -> u32 {
        1
    }

    fn description(&self) -> &str {
        "Initial unified schema"
    }

    fn run(&self, conn: &Connection) -> Result<()> {
        // Main memory records table
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_records (
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
                revisions TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_memories_workspace ON memory_records (workspace_id);
            CREATE INDEX IF NOT EXISTS idx_memories_path ON memory_records (workspace_id, path);
            CREATE INDEX IF NOT EXISTS idx_memories_parent ON memory_records (parent_id);"
        )?;

        // Column repair for memory_records
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
        ];

        for (col, def) in memory_columns {
            if !table_has_column(conn, "memory_records", col)? {
                conn.execute(&format!("ALTER TABLE memory_records ADD COLUMN {} {}", col, def), [])?;
            }
        }

        // Belief states
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS belief_states (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                beliefs TEXT NOT NULL DEFAULT '[]',
                updated_at DATETIME NOT NULL
            );"
        )?;

        // Session tokens
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS session_tokens (
                token TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                created_at DATETIME NOT NULL,
                expires_at DATETIME NOT NULL
            );"
        )?;

        // Checkpoints
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS checkpoint_records (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                name TEXT NOT NULL,
                data TEXT NOT NULL,
                created_at DATETIME NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_checkpoints_lookup ON checkpoint_records (workspace_id, task_id, name);"
        )?;

        // Knowledge Graph tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                properties TEXT,
                language_family TEXT
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
                is_inferred INTEGER DEFAULT 0,
                source_language TEXT,
                target_language TEXT,
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
            CREATE INDEX IF NOT EXISTS idx_memory_entities_entity ON memory_entities(entity_id);"
        )?;

        // Column repair for entities and relations
        if !table_has_column(conn, "entities", "language_family")? {
            conn.execute("ALTER TABLE entities ADD COLUMN language_family TEXT", [])?;
        }

        let relation_columns = [
            ("weight", "REAL DEFAULT 1.0"),
            ("confidence_score", "REAL DEFAULT 1.0"),
            ("contradicts_edge_id", "TEXT"),
            ("is_inferred", "INTEGER DEFAULT 0"),
            ("source_language", "TEXT"),
            ("target_language", "TEXT"),
            ("created_at", "DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))"),
            ("updated_at", "DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))"),
        ];

        for (col, def) in relation_columns {
            if !table_has_column(conn, "relations", col)? {
                conn.execute(&format!("ALTER TABLE relations ADD COLUMN {} {}", col, def), [])?;
            }
        }

        // Integrity chain & Timeline
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_chain (
                id TEXT PRIMARY KEY,
                prev_hash TEXT,
                content_hash TEXT NOT NULL,
                created_at DATETIME DEFAULT (strftime('%Y-%m-%d %H:%M:%f', 'now'))
            );
            CREATE INDEX IF NOT EXISTS idx_memory_chain_created ON memory_chain(created_at);

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
            CREATE INDEX IF NOT EXISTS idx_timeline_sequence ON timeline_events(sequence);"
        )?;

        // Column repair for timeline_events
        let timeline_columns = [
            ("summary", "TEXT"),
            ("details", "TEXT"),
            ("agent_id", "TEXT"),
            ("prev_hash", "TEXT"),
            ("curr_hash", "TEXT DEFAULT ''"),
        ];

        for (col, def) in timeline_columns {
            if !table_has_column(conn, "timeline_events", col)? {
                conn.execute(&format!("ALTER TABLE timeline_events ADD COLUMN {} {}", col, def), [])?;
            }
        }

        // Panel UI & Other utility tables
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS panel_bookmarks (
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
            );"
        )?;

        Ok(())
    }
}

pub struct MigrationV2ColumnarIndices;

impl Migration for MigrationV2ColumnarIndices {
    fn version(&self) -> u32 {
        2
    }

    fn description(&self) -> &str {
        "Implement columnar indices (composite covering indices)"
    }

    fn run(&self, conn: &Connection) -> Result<()> {
        // Covering index for common workspace filtering with level and created_at ordering
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_columnar_workspace_level
             ON memory_records (workspace_id, level, created_at, id)",
            [],
        )?;

        // Covering index for path-based lookups within workspace
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_columnar_path
             ON memory_records (workspace_id, path, updated_at)",
            [],
        )?;

        // Index for hierarchical navigation
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_columnar_parent
             ON memory_records (workspace_id, parent_id, level)",
            [],
        )?;

        // Index for cluster analysis
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_columnar_cluster
             ON memory_records (workspace_id, cluster_id, level)",
            [],
        )?;

        // Integrity chain sequence optimization
        if !table_has_column(conn, "memory_chain", "workspace_id")? {
            conn.execute("ALTER TABLE memory_chain ADD COLUMN workspace_id TEXT", [])?;
        }

        Ok(())
    }
}
