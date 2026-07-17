//! SQLite vector store schema implementation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;
use anyhow::Result;
use rusqlite::{params, Connection};

use super::{vector, VecSqliteMemoryStore};

impl SchemaInitializer for VecSqliteMemoryStore {
    fn init_schema(&self) -> Result<()> {
        match tokio::runtime::Handle::try_current() {
            Ok(_) => tokio::task::block_in_place(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .build()
                    .map_err(|e| anyhow::anyhow!("failed to create temporary runtime: {}", e))?;
                rt.block_on(self.init_schema_async())
            }),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new()
                    .map_err(|e| anyhow::anyhow!("failed to create tokio runtime: {}", e))?;
                runtime.block_on(self.init_schema_async())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexAction {
    None,
    Invalidated(usize),
}

impl VecSqliteMemoryStore {
    pub async fn init_schema_async(&self) -> Result<()> {
        let project_id = self.project_id.clone();

        ConnectionManager::global()
            .with_conn(&project_id, move |conn| {
                // Run unified migrations
                let mut manager = crate::storage::MigrationManager::new();
                manager.add_migration(crate::storage::migrations::MigrationV1InitialSchema);
                manager.add_migration(crate::storage::migrations::MigrationV2ColumnarIndices);
                manager.add_migration(crate::storage::migrations::MigrationV3UnifiedExtensions);
                manager.add_migration(crate::storage::migrations::MigrationV4UnifiedIsolation);
                manager.add_migration(crate::storage::migrations::MigrationV5SessionTokensId);
                manager.add_migration(crate::storage::migrations::MigrationV6RecoverySystem);
                manager.add_migration(crate::storage::migrations::MigrationV7EmbeddingModelMeta);
                manager.run_migrations(conn)?;

                // Run automatic vector migration
                Self::migrate_embeddings_on_startup(conn)?;

                Ok(())
            })
            .await?;

        // Retrieve the old model name from the database (if any) before the change
        let project_id_c = self.project_id.clone();
        let old_model = ConnectionManager::global()
            .with_conn(&project_id_c, move |conn| {
                let mut stmt = conn.prepare("SELECT value FROM embedding_model_meta WHERE key = 'active'")?;
                let mut rows = stmt.query([])?;
                if let Some(row) = rows.next()? {
                    let value: String = row.get(0)?;
                    Ok(value)
                } else {
                    Ok("<none>".to_string())
                }
            })
            .await
            .unwrap_or_else(|_| "<none>".to_string());

        // Get the active model name from the environment
        let active_model = crate::embedding::EmbedderConfig::from_env()
            .active_model_name()
            .unwrap_or_else(|| "noop".to_string());

        let active_model_c = active_model.clone();
        let project_id_c2 = self.project_id.clone();
        let reindex_action = ConnectionManager::global()
            .with_conn(&project_id_c2, move |conn| {
                let rt = tokio::runtime::Handle::current();
                rt.block_on(Self::check_and_handle_embedding_model_change(conn, &active_model_c))
            })
            .await?;

        if let ReindexAction::Invalidated(n) = reindex_action {
            tracing::warn!(
                "Embedding model changed from {} to {}: invalidated {} vectors. Run POST /memory/reindex to rebuild.",
                old_model,
                active_model,
                n
            );
        }

        Ok(())
    }

    pub async fn check_and_handle_embedding_model_change(
        conn: &Connection,
        active_model: &str,
    ) -> Result<ReindexAction> {
        let mut stmt = conn.prepare("SELECT value FROM embedding_model_meta WHERE key = 'active'")?;
        let mut rows = stmt.query([])?;
        let old_model: Option<String> = if let Some(row) = rows.next()? {
            Some(row.get(0)?)
        } else {
            None
        };

        if old_model.as_deref() != Some(active_model) {
            let n_rows = conn.execute(
                "UPDATE memory_records SET embedding = NULL WHERE embedding IS NOT NULL",
                [],
            )?;
            conn.execute("DELETE FROM memory_embeddings", [])?;
            conn.execute(
                "INSERT OR REPLACE INTO embedding_model_meta (key, value) VALUES ('active', ?1)",
                params![active_model],
            )?;
            Ok(ReindexAction::Invalidated(n_rows))
        } else {
            Ok(ReindexAction::None)
        }
    }

    fn migrate_embeddings_on_startup(conn: &Connection) -> Result<()> {
        // 1. Check if we already migrated embeddings (meaning memory_embeddings is not empty)
        let current_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_embeddings", (), |row| {
                row.get(0)
            })?;

        if current_count > 0 {
            return Ok(());
        }

        // 2. Query all existing memories with non-null embeddings
        let mut select_stmt = conn.prepare(
            "SELECT id, workspace_id, embedding FROM memory_records WHERE embedding IS NOT NULL",
        )?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Create mock tables matching the schema needed for our tests
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_records (
                id TEXT PRIMARY KEY,
                embedding BLOB
            );
            CREATE TABLE IF NOT EXISTS memory_embeddings (
                id TEXT PRIMARY KEY,
                workspace_id TEXT,
                embedding BLOB
            );
            CREATE TABLE IF NOT EXISTS embedding_model_meta (
                key TEXT PRIMARY KEY,
                value TEXT
            );"
        ).unwrap();
        conn
    }

    #[tokio::test]
    async fn test_model_change_first_time() {
        let conn = setup_test_db();

        // Insert a record with an embedding to check invalidation
        conn.execute(
            "INSERT INTO memory_records (id, embedding) VALUES ('mem_1', X'1234')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO memory_embeddings (id, workspace_id, embedding) VALUES ('mem_1', 'ws_1', X'5678')",
            []
        ).unwrap();

        // Since it's the first time, no 'active' key exists in embedding_model_meta
        let action = VecSqliteMemoryStore::check_and_handle_embedding_model_change(&conn, "qwen3-coder")
            .await
            .unwrap();

        assert_eq!(action, ReindexAction::Invalidated(1));

        // Check that embedding is now NULL
        let embedding: Option<Vec<u8>> = conn.query_row(
            "SELECT embedding FROM memory_records WHERE id = 'mem_1'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(embedding.is_none());

        // Check that memory_embeddings was cleared
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_embeddings",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 0);

        // Check that model was saved as active
        let saved_model: String = conn.query_row(
            "SELECT value FROM embedding_model_meta WHERE key = 'active'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(saved_model, "qwen3-coder");
    }

    #[tokio::test]
    async fn test_model_change_same_model() {
        let conn = setup_test_db();

        // Pre-save the model
        conn.execute(
            "INSERT INTO embedding_model_meta (key, value) VALUES ('active', 'qwen3-coder')",
            []
        ).unwrap();

        // Insert a record with an embedding
        conn.execute(
            "INSERT INTO memory_records (id, embedding) VALUES ('mem_1', X'1234')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO memory_embeddings (id, workspace_id, embedding) VALUES ('mem_1', 'ws_1', X'5678')",
            []
        ).unwrap();

        // Same model, should return ReindexAction::None
        let action = VecSqliteMemoryStore::check_and_handle_embedding_model_change(&conn, "qwen3-coder")
            .await
            .unwrap();

        assert_eq!(action, ReindexAction::None);

        // Check that embedding was NOT nullified
        let embedding: Option<Vec<u8>> = conn.query_row(
            "SELECT embedding FROM memory_records WHERE id = 'mem_1'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(embedding.is_some());

        // Check that memory_embeddings was NOT cleared
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_embeddings",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_model_change_different_model() {
        let conn = setup_test_db();

        // Pre-save an old model name
        conn.execute(
            "INSERT INTO embedding_model_meta (key, value) VALUES ('active', 'old-model')",
            []
        ).unwrap();

        // Insert a record with an embedding
        conn.execute(
            "INSERT INTO memory_records (id, embedding) VALUES ('mem_1', X'1234')",
            []
        ).unwrap();
        conn.execute(
            "INSERT INTO memory_embeddings (id, workspace_id, embedding) VALUES ('mem_1', 'ws_1', X'5678')",
            []
        ).unwrap();

        // Different model, should invalidate and return Invalidated(1)
        let action = VecSqliteMemoryStore::check_and_handle_embedding_model_change(&conn, "qwen3-coder")
            .await
            .unwrap();

        assert_eq!(action, ReindexAction::Invalidated(1));

        // Check that embedding is now NULL
        let embedding: Option<Vec<u8>> = conn.query_row(
            "SELECT embedding FROM memory_records WHERE id = 'mem_1'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert!(embedding.is_none());

        // Check that memory_embeddings was cleared
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memory_embeddings",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(count, 0);

        // Check that the new model was saved
        let saved_model: String = conn.query_row(
            "SELECT value FROM embedding_model_meta WHERE key = 'active'",
            [],
            |r| r.get(0)
        ).unwrap();
        assert_eq!(saved_model, "qwen3-coder");
    }
}
