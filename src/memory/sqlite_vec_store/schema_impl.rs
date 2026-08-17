//! SQLite vector store schema implementation
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::codebase::connection_manager::ConnectionManager;
use crate::ports::outbound::schema_init::SchemaInitializer;
use anyhow::Result;
use rusqlite::{params, Connection};
use std::sync::Arc;

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
    /// Init schema async.
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
                manager.add_migration(crate::storage::migrations::MigrationV8EntityGraphSnapshots);
                manager.add_migration(crate::storage::migrations::MigrationV9EmbeddingStatus);
                manager.add_migration(crate::storage::migrations::MigrationV10Embeddings768);
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
                let mut stmt =
                    conn.prepare("SELECT value FROM embedding_model_meta WHERE key = 'active'")?;
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
                rt.block_on(Self::check_and_handle_embedding_model_change(
                    conn,
                    &active_model_c,
                ))
            })
            .await?;

        if let ReindexAction::Invalidated(n) = reindex_action {
            tracing::warn!(
                "Embedding model changed from {} to {}: invalidated {} vectors. Run POST /memory/reindex to rebuild.",
                old_model,
                active_model,
                n
            );
            if n > 0 {
                let store_clone = self.clone();
                tokio::spawn(async move {
                    match store_clone.reindex_null_embeddings_background().await {
                        Ok(count) => {
                            tracing::info!(
                                "Background reindexing processed {} records successfully.",
                                count
                            );
                        }
                        Err(e) => {
                            tracing::error!("Background reindexing failed: {}", e);
                        }
                    }
                });
            }
        }

        Ok(())
    }

    /// Reindex null embeddings background.
    pub async fn reindex_null_embeddings_background(&self) -> Result<usize> {
        self.reindex_null_embeddings_background_with_limit(None)
            .await
    }

    /// Reindex null embeddings background with limit.
    pub async fn reindex_null_embeddings_background_with_limit(
        &self,
        limit: Option<usize>,
    ) -> Result<usize> {
        let embedder = match crate::embedding::build_embedder_from_env().await {
            Ok(emb) => emb,
            Err(e) => {
                tracing::error!("Failed to build embedder for background reindexing: {}", e);
                return Err(anyhow::anyhow!("Embedder build failed: {}", e));
            }
        };

        let project_id_c = self.project_id.clone();
        let records = ConnectionManager::global()
            .with_conn(&project_id_c, move |conn| {
                let sql = if let Some(lim) = limit {
                    format!(
                        "SELECT id, workspace_id, path, content, metadata, X'' AS embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv, embedding_status, embedding_attempts FROM memory_records WHERE embedding IS NULL AND (embedding_status IS NULL OR embedding_status = 'pending' OR embedding_status = 'retry') LIMIT {}",
                        lim
                    )
                } else {
                    "SELECT id, workspace_id, path, content, metadata, X'' AS embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv, embedding_status, embedding_attempts FROM memory_records WHERE embedding IS NULL AND (embedding_status IS NULL OR embedding_status = 'pending' OR embedding_status = 'retry')".to_string()
                };
                let mut stmt = conn.prepare(&sql)?;
                let mut rows = stmt.query([])?;
                let mut records = Vec::new();
                while let Some(row) = rows.next()? {
                    records.push(Self::deserialize_record(row)?);
                }
                Ok(records)
            })
            .await?;

        let mut decrypted_records = Vec::new();
        for mut record in records {
            if let Err(e) =
                crate::memory::sqlite_store::SqliteMemoryStore::decrypt_record(&mut record)
            {
                tracing::warn!(
                    "Failed to decrypt record {} during background reindexing: {}",
                    record.id,
                    e
                );
                continue;
            }
            decrypted_records.push(record);
        }

        let total_records = decrypted_records.len();
        if total_records == 0 {
            return Ok(0);
        }

        tracing::info!(
            "Starting background reindexing for {} records.",
            total_records
        );

        let batch_size = 10;
        let mut processed_count = 0;
        let mut success_count = 0;
        let mut fail_count = 0;

        for chunk in decrypted_records.chunks(batch_size) {
            let mut tasks = Vec::new();
            for record in chunk {
                let embedder = Arc::clone(&embedder);
                let content = record.content.clone();
                let record_id = record.id.clone();
                tasks.push(async move {
                    let res: Result<Vec<f32>, crate::embedding::EmbeddingError> =
                        tokio::time::timeout(
                            std::time::Duration::from_secs(60),
                            embedder.encode(&content),
                        )
                        .await
                        .map_err(|_| {
                            crate::embedding::EmbeddingError::Network("timeout".to_string())
                        })
                        .and_then(|r| r);

                    match res {
                        Ok(emb) => Ok((record_id, emb)),
                        Err(e) => Err((record_id, format!("{}", e))),
                    }
                });
            }

            let results = futures_util::future::join_all(tasks).await;

            for res in results {
                match res {
                    Ok((record_id, embedding)) => {
                        let project_id_c = self.project_id.clone();
                        let record_to_update = chunk.iter().find(|r| r.id == record_id).cloned();
                        if let Some(record) = record_to_update {
                            let workspace_id = record.workspace_id.clone();
                            let embedding_c: Vec<f32> = embedding.clone();
                            let record_id_for_db = record_id.clone();
                            let update_res = ConnectionManager::global()
                                .with_conn(&project_id_c, move |conn| {
                                    let qjl_enabled = {
                                        let threshold = Self::configured_qjl_threshold();
                                        let current_vectors: usize = conn.query_row(
                                            "SELECT COUNT(*) FROM memory_embeddings WHERE workspace_id = ?",
                                            params![workspace_id],
                                            |row| row.get(0)
                                        ).unwrap_or(0);
                                        current_vectors >= threshold
                                    };

                                    let embedding_blob = if qjl_enabled {
                                        vector::serialize_embedding_qjl(&embedding_c)
                                    } else {
                                        vector::serialize_embedding(&embedding_c)
                                    };

                                    // Update memory_records
                                    conn.execute(
                                        "UPDATE memory_records SET embedding = ?1, updated_at = ?2, embedding_status = 'completed', embedding_attempts = 0 WHERE id = ?3",
                                        params![
                                            embedding_blob,
                                            chrono::Utc::now().to_rfc3339(),
                                            record_id_for_db.as_str()
                                        ],
                                    )?;

                                    // Update vector table (delete + insert is reliable for vec0)
                                    conn.execute(
                                        "DELETE FROM memory_embeddings WHERE id = ?1",
                                        params![record_id_for_db.as_str()],
                                    )?;
                                    conn.execute(
                                        "DELETE FROM memory_embeddings_768 WHERE id = ?1",
                                        params![record_id_for_db.as_str()],
                                    )?;
                                    let embedding_json =
                                        serde_json::to_string(&embedding_c).unwrap_or_default();
                                    let table_name = if embedding_c.len() == 768 {
                                        "memory_embeddings_768"
                                    } else {
                                        "memory_embeddings"
                                    };
                                    let sql = format!(
                                        "INSERT INTO {}(id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))",
                                        table_name
                                    );
                                    let inserted = conn.execute(
                                        &sql,
                                        params![
                                            record_id_for_db.as_str(),
                                            workspace_id.as_str(),
                                            embedding_json
                                        ],
                                    )?;
                                    if inserted == 0 {
                                        anyhow::bail!(
                                            "vector table insert affected 0 rows for {}",
                                            record_id_for_db
                                        );
                                    }
                                    let count_sql = format!(
                                        "SELECT COUNT(*) FROM {} WHERE id = ?1",
                                        table_name
                                    );
                                    let vec_rows: i64 = conn.query_row(
                                        &count_sql,
                                        params![record_id_for_db.as_str()],
                                        |r| r.get(0),
                                    )?;
                                    if vec_rows == 0 {
                                        anyhow::bail!(
                                            "vector table missing row after insert for {}",
                                            record_id_for_db
                                        );
                                    }

                                    Ok(())
                                })
                                .await;

                            if let Err(e) = update_res {
                                tracing::error!(
                                    "Failed to update embedded record {} in database: {}",
                                    record_id,
                                    e
                                );
                                fail_count += 1;
                            } else {
                                success_count += 1;
                            }
                        } else {
                            tracing::error!("Record {} not found in chunk", record_id);
                            fail_count += 1;
                        }
                    }
                    Err((record_id, err_details)) => {
                        tracing::error!(
                            "Failed to generate embedding for record {}: {}",
                            record_id,
                            err_details
                        );
                        let project_id_c = self.project_id.clone();
                        let record_id_for_db = record_id.clone();
                        let chunk_rec = chunk.iter().find(|r| r.id == record_id);
                        let attempts = chunk_rec.map(|r| r.embedding_attempts + 1).unwrap_or(1);
                        let new_status = if attempts >= 5 { "failed" } else { "retry" };

                        let _ = ConnectionManager::global()
                            .with_conn(&project_id_c, move |conn| {
                                conn.execute(
                                    "UPDATE memory_records SET embedding_status = ?1, embedding_attempts = ?2, updated_at = ?3 WHERE id = ?4",
                                    params![
                                        new_status,
                                        attempts,
                                        chrono::Utc::now().to_rfc3339(),
                                        record_id_for_db.as_str()
                                    ],
                                )?;
                                Ok(())
                            })
                            .await;

                        fail_count += 1;
                    }
                }
            }

            processed_count += chunk.len();
            let prev_hundred = (processed_count - chunk.len()) / 100;
            let curr_hundred = processed_count / 100;
            if curr_hundred > prev_hundred || processed_count == total_records {
                tracing::info!(
                    "Reindexación en background: {}/{} registros procesados.",
                    processed_count,
                    total_records
                );
            }
        }

        if fail_count > 0 {
            tracing::warn!(
                "Background reindexing completed with errors. Success: {}, Failed: {}",
                success_count,
                fail_count
            );
        } else {
            tracing::info!(
                "Background reindexing completed successfully. Reindexed {} records.",
                success_count
            );
        }

        Ok(success_count)
    }

    /// Check and handle embedding model change.
    pub async fn check_and_handle_embedding_model_change(
        conn: &Connection,
        active_model: &str,
    ) -> Result<ReindexAction> {
        let mut stmt =
            conn.prepare("SELECT value FROM embedding_model_meta WHERE key = 'active'")?;
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
            );",
        )
        .unwrap();
        conn
    }

    #[tokio::test]
    async fn test_model_change_first_time() {
        let conn = setup_test_db();

        // Insert a record with an embedding to check invalidation
        conn.execute(
            "INSERT INTO memory_records (id, embedding) VALUES ('mem_1', X'1234')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_embeddings (id, workspace_id, embedding) VALUES ('mem_1', 'ws_1', X'5678')",
            []
        ).unwrap();

        // Since it's the first time, no 'active' key exists in embedding_model_meta
        let action =
            VecSqliteMemoryStore::check_and_handle_embedding_model_change(&conn, "qwen3-coder")
                .await
                .unwrap();

        assert_eq!(action, ReindexAction::Invalidated(1));

        // Check that embedding is now NULL
        let embedding: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM memory_records WHERE id = 'mem_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(embedding.is_none());

        // Check that memory_embeddings was cleared
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Check that model was saved as active
        let saved_model: String = conn
            .query_row(
                "SELECT value FROM embedding_model_meta WHERE key = 'active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved_model, "qwen3-coder");
    }

    #[tokio::test]
    async fn test_model_change_same_model() {
        let conn = setup_test_db();

        // Pre-save the model
        conn.execute(
            "INSERT INTO embedding_model_meta (key, value) VALUES ('active', 'qwen3-coder')",
            [],
        )
        .unwrap();

        // Insert a record with an embedding
        conn.execute(
            "INSERT INTO memory_records (id, embedding) VALUES ('mem_1', X'1234')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_embeddings (id, workspace_id, embedding) VALUES ('mem_1', 'ws_1', X'5678')",
            []
        ).unwrap();

        // Same model, should return ReindexAction::None
        let action =
            VecSqliteMemoryStore::check_and_handle_embedding_model_change(&conn, "qwen3-coder")
                .await
                .unwrap();

        assert_eq!(action, ReindexAction::None);

        // Check that embedding was NOT nullified
        let embedding: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM memory_records WHERE id = 'mem_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(embedding.is_some());

        // Check that memory_embeddings was NOT cleared
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_model_change_different_model() {
        let conn = setup_test_db();

        // Pre-save an old model name
        conn.execute(
            "INSERT INTO embedding_model_meta (key, value) VALUES ('active', 'old-model')",
            [],
        )
        .unwrap();

        // Insert a record with an embedding
        conn.execute(
            "INSERT INTO memory_records (id, embedding) VALUES ('mem_1', X'1234')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memory_embeddings (id, workspace_id, embedding) VALUES ('mem_1', 'ws_1', X'5678')",
            []
        ).unwrap();

        // Different model, should invalidate and return Invalidated(1)
        let action =
            VecSqliteMemoryStore::check_and_handle_embedding_model_change(&conn, "qwen3-coder")
                .await
                .unwrap();

        assert_eq!(action, ReindexAction::Invalidated(1));

        // Check that embedding is now NULL
        let embedding: Option<Vec<u8>> = conn
            .query_row(
                "SELECT embedding FROM memory_records WHERE id = 'mem_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(embedding.is_none());

        // Check that memory_embeddings was cleared
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Check that the new model was saved
        let saved_model: String = conn
            .query_row(
                "SELECT value FROM embedding_model_meta WHERE key = 'active'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved_model, "qwen3-coder");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_reindex_null_embeddings_background() {
        use crate::memory::store::MemoryStore;
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        // Clear any stale embedding env vars from previous tests
        // NOTE: XAVIER_EMBEDDER must be cleared too — other tests (e.g. qmd offline)
        // set it to "disabled" which causes build_embedder_from_env() to return NoopEmbedder
        for key in &[
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            "XAVIER_EMBEDDING_URL",
            "OPENAI_API_KEY",
            "XAVIER_EMBEDDING_MODEL",
            "XAVIER_EMBEDDER",
            "XAVIER_EMBEDDING_LOCAL_URL",
        ] {
            std::env::remove_var(key);
        }

        // Setup a mock API server using mockito
        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        // Create a temporary database using VecSqliteMemoryStore
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_reindex.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };

        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        // Insert a record with NULL embedding (no env vars set yet → no auto-embedding)
        let record = crate::memory::store::MemoryRecord {
            id: "test_mem_1".to_string(),
            workspace_id: "test_ws_1".to_string(),
            path: "test/path".to_string(),
            content: "Hello world".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        };

        store.put(record).await.unwrap();

        // Now set env vars and mock for the reindex step
        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
        std::env::set_var(
            "XAVIER_EMBEDDING_URL",
            format!("{}/v1/embeddings", mock_url),
        );
        std::env::set_var("OPENAI_API_KEY", "test-api-key");
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "test-model");

        // Mock the embedding API response
        let _mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "data": [
                    {
                        "embedding": [0.1, 0.2, 0.3]
                    }
                ]
            }"#,
            )
            .create_async()
            .await;

        // Manually update to set embedding = NULL in the DB to simulate model change invalidation
        ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                conn.execute("UPDATE memory_records SET embedding = NULL", [])
                    .unwrap();
                conn.execute("DELETE FROM memory_embeddings", []).unwrap();
                Ok(())
            })
            .await
            .unwrap();

        // Verify that embedding is NULL before background reindexing
        let is_null = ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                let embedding: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT embedding FROM memory_records WHERE id = 'test_mem_1'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                Ok(embedding.is_none())
            })
            .await
            .unwrap();
        assert!(is_null);

        // Re-assert env vars right before reindex
        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
        std::env::set_var(
            "XAVIER_EMBEDDING_URL",
            format!("{}/v1/embeddings", server.url()),
        );
        std::env::set_var("OPENAI_API_KEY", "test-api-key");
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "test-model");

        // Run the background reindexing
        let reindex_result = store.reindex_null_embeddings_background().await;
        if let Err(ref e) = reindex_result {
            panic!("reindex failed: {}", e);
        }
        let success_count = reindex_result.unwrap();
        assert_eq!(
            success_count, 1,
            "Expected exactly 1 record to be successfully reindexed"
        );

        // Verify that embedding was successfully updated
        let (updated_embedding, has_vector_row) = ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                let embedding_blob: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT embedding FROM memory_records WHERE id = 'test_mem_1'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();

                let count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM memory_embeddings WHERE id = 'test_mem_1'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();

                Ok((embedding_blob, count > 0))
            })
            .await
            .unwrap();

        assert!(updated_embedding.is_some());
        assert!(has_vector_row);

        // Check if the deserialized embedding has the correct values
        let floats = vector::deserialize_embedding(&updated_embedding.unwrap());
        assert_eq!(floats, vec![0.1f32, 0.2f32, 0.3f32]);

        // Cleanup env vars
        std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
        std::env::remove_var("XAVIER_EMBEDDING_URL");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_reindex_null_embeddings_background_with_limit_batches() {
        use crate::memory::store::MemoryStore;
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        for key in &[
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            "XAVIER_EMBEDDING_URL",
            "OPENAI_API_KEY",
            "XAVIER_EMBEDDING_MODEL",
            "XAVIER_EMBEDDER",
            "XAVIER_EMBEDDING_LOCAL_URL",
        ] {
            std::env::remove_var(key);
        }

        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_reindex_limit.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };

        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        // Insert 5 records with NULL embedding
        for i in 1..=5 {
            let record = crate::memory::store::MemoryRecord {
                id: format!("test_batch_mem_{}", i),
                workspace_id: "test_ws_1".to_string(),
                path: format!("test/path/{}", i),
                content: format!("Content number {}", i),
                metadata: serde_json::json!({}),
                embedding: vec![],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                ..Default::default()
            };
            store.put(record).await.unwrap();
        }

        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
        std::env::set_var(
            "XAVIER_EMBEDDING_URL",
            format!("{}/v1/embeddings", mock_url),
        );
        std::env::set_var("OPENAI_API_KEY", "test-api-key");
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "test-model");

        let _mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                "data": [
                    {
                        "embedding": [0.1, 0.2, 0.3]
                    }
                ]
            }"#,
            )
            .expect_at_least(5)
            .create_async()
            .await;

        // Force embeddings to NULL
        ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                conn.execute("UPDATE memory_records SET embedding = NULL", [])
                    .unwrap();
                conn.execute("DELETE FROM memory_embeddings", []).unwrap();
                Ok(())
            })
            .await
            .unwrap();

        // Batch 1: process 2 records
        let res1 = store
            .reindex_null_embeddings_background_with_limit(Some(2))
            .await
            .unwrap();
        assert_eq!(res1, 2);

        // Batch 2: process next 2 records
        let res2 = store
            .reindex_null_embeddings_background_with_limit(Some(2))
            .await
            .unwrap();
        assert_eq!(res2, 2);

        // Batch 3: process remaining 1 record
        let res3 = store
            .reindex_null_embeddings_background_with_limit(Some(2))
            .await
            .unwrap();
        assert_eq!(res3, 1);

        // Batch 4: no more records left
        let res4 = store
            .reindex_null_embeddings_background_with_limit(Some(2))
            .await
            .unwrap();
        assert_eq!(res4, 0);

        std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
        std::env::remove_var("XAVIER_EMBEDDING_URL");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_reindex_null_embeddings_background_with_errors() {
        use crate::memory::store::MemoryStore;
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        for key in &[
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            "XAVIER_EMBEDDING_URL",
            "OPENAI_API_KEY",
            "XAVIER_EMBEDDING_MODEL",
            "XAVIER_EMBEDDER",
            "XAVIER_EMBEDDING_LOCAL_URL",
        ] {
            std::env::remove_var(key);
        }

        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_reindex_err.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };

        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        let record = crate::memory::store::MemoryRecord {
            id: "test_mem_err_1".to_string(),
            workspace_id: "test_ws_1".to_string(),
            path: "test/path".to_string(),
            content: "Hello world".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        };

        store.put(record).await.unwrap();

        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
        std::env::set_var(
            "XAVIER_EMBEDDING_URL",
            format!("{}/v1/embeddings", mock_url),
        );
        std::env::set_var("OPENAI_API_KEY", "test-api-key");
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "test-model");

        // Mock a 500 error from embedding endpoint to trigger failure
        let _mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(500)
            .create_async()
            .await;

        ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                conn.execute("UPDATE memory_records SET embedding = NULL", [])
                    .unwrap();
                conn.execute("DELETE FROM memory_embeddings", []).unwrap();
                Ok(())
            })
            .await
            .unwrap();

        let reindex_result = store.reindex_null_embeddings_background().await;
        let success_count = reindex_result.unwrap();
        assert_eq!(
            success_count, 0,
            "Expected 0 records to be successfully reindexed on API error"
        );

        // Verify that embedding_attempts was incremented and status set to 'retry'
        let (status, attempts): (String, u32) = ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                let row = conn.query_row(
                    "SELECT embedding_status, embedding_attempts FROM memory_records WHERE id = 'test_mem_err_1'",
                    [],
                    |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<u32>>(1)?.unwrap_or(0))),
                ).unwrap();
                Ok(row)
            })
            .await
            .unwrap();

        assert_eq!(status, "retry");
        assert_eq!(attempts, 1);

        std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
        std::env::remove_var("XAVIER_EMBEDDING_URL");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_dead_letter_isolation_after_max_attempts() {
        use crate::memory::store::MemoryStore;
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        for key in &[
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            "XAVIER_EMBEDDING_URL",
            "OPENAI_API_KEY",
            "XAVIER_EMBEDDING_MODEL",
            "XAVIER_EMBEDDER",
            "XAVIER_EMBEDDING_LOCAL_URL",
        ] {
            std::env::remove_var(key);
        }

        let mut server = mockito::Server::new_async().await;
        let mock_url = server.url();

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_dead_letter.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };

        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        let record = crate::memory::store::MemoryRecord {
            id: "test_dead_letter_1".to_string(),
            workspace_id: "test_ws_1".to_string(),
            path: "test/dead_letter".to_string(),
            content: "Dead letter content".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        };

        store.put(record).await.unwrap();

        std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "cloud");
        std::env::set_var(
            "XAVIER_EMBEDDING_URL",
            format!("{}/v1/embeddings", mock_url),
        );
        std::env::set_var("OPENAI_API_KEY", "test-api-key");
        std::env::set_var("XAVIER_EMBEDDING_MODEL", "test-model");

        let _mock = server
            .mock("POST", "/v1/embeddings")
            .with_status(500)
            .expect(5)
            .create_async()
            .await;

        ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                conn.execute("UPDATE memory_records SET embedding = NULL", [])
                    .unwrap();
                conn.execute("DELETE FROM memory_embeddings", []).unwrap();
                Ok(())
            })
            .await
            .unwrap();

        // Run 5 attempts to reach dead-letter 'failed' status
        for _ in 0..5 {
            let _ = store.reindex_null_embeddings_background().await;
        }

        let (status, attempts): (String, u32) = ConnectionManager::global()
            .with_conn(&store.project_id, |conn| {
                let row = conn.query_row(
                    "SELECT embedding_status, embedding_attempts FROM memory_records WHERE id = 'test_dead_letter_1'",
                    [],
                    |r| Ok((r.get::<_, Option<String>>(0)?.unwrap_or_default(), r.get::<_, Option<u32>>(1)?.unwrap_or(0))),
                ).unwrap();
                Ok(row)
            })
            .await
            .unwrap();

        assert_eq!(status, "failed");
        assert_eq!(attempts, 5);

        // 6th run should not attempt to reindex the failed record
        let reindex_result = store.reindex_null_embeddings_background().await.unwrap();
        assert_eq!(
            reindex_result, 0,
            "Failed record must be skipped on subsequent reindex ticks"
        );

        std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
        std::env::remove_var("XAVIER_EMBEDDING_URL");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("XAVIER_EMBEDDING_MODEL");
    }

    #[tokio::test]
    async fn test_is_superset() {
        use crate::memory::sqlite_vec_store::store_impl::is_superset;
        assert!(is_superset("hello world", "hello"));
        assert!(is_superset("hello", "hello"));
        assert!(is_superset("hello", ""));
        assert!(is_superset("", ""));
        assert!(!is_superset("hello", "world"));
        assert!(!is_superset("", "hello"));
        assert!(!is_superset("abc", "bcd"));
    }

    #[tokio::test]
    async fn test_superset_merges_revisions() {
        use crate::memory::store::MemoryStore;
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_superset.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };
        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        store
            .set_dedup_settings(crate::settings::types::DedupSettings {
                enabled: true,
                threshold: 0.90,
                scope: crate::settings::types::DedupScope::PathExact,
                max_revisions: 5,
            })
            .await;

        let rec = crate::memory::store::MemoryRecord {
            id: "test_rec".to_string(),
            workspace_id: "test_ws".to_string(),
            path: "test/path".to_string(),
            content: "Initial content".to_string(),
            embedding: vec![0.1, 0.2, 0.3],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            revisions: vec![crate::memory::store::MemoryRevision {
                revision: 1,
                recorded_at: chrono::Utc::now(),
                path: "test/path".to_string(),
                content: "Initial content".to_string(),
                metadata: serde_json::json!({}),
            }],
            ..Default::default()
        };
        store.put(rec.clone()).await.unwrap();

        let mut current_content = "Initial content".to_string();
        for i in 1..=10 {
            current_content = format!("{} and more {}", current_content, i);
            let merge_rec = crate::memory::store::MemoryRecord {
                id: format!("merge_{}", i),
                workspace_id: "test_ws".to_string(),
                path: "test/path".to_string(),
                content: current_content.clone(),
                embedding: vec![0.1, 0.2, 0.3],
                metadata: serde_json::json!({ "dedup": true }),
                ..Default::default()
            };
            store.put(merge_rec).await.unwrap();
        }

        let fetched = store.get("test_ws", "test/path").await.unwrap().unwrap();
        assert_eq!(
            fetched.revisions.len(),
            1,
            "Revisions count must remain exactly 1"
        );
        assert_eq!(fetched.content, current_content);
    }

    #[tokio::test]
    async fn test_different_merges_revisions_cap() {
        use crate::memory::store::MemoryStore;
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_different.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };
        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        store
            .set_dedup_settings(crate::settings::types::DedupSettings {
                enabled: true,
                threshold: 0.90,
                scope: crate::settings::types::DedupScope::PathExact,
                max_revisions: 5,
            })
            .await;

        let rec = crate::memory::store::MemoryRecord {
            id: "test_rec".to_string(),
            workspace_id: "test_ws".to_string(),
            path: "test/path".to_string(),
            content: "Initial content".to_string(),
            embedding: vec![0.1, 0.2, 0.3],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        };
        store.put(rec).await.unwrap();

        for i in 1..=10 {
            let merge_rec = crate::memory::store::MemoryRecord {
                id: format!("merge_{}", i),
                workspace_id: "test_ws".to_string(),
                path: "test/path".to_string(),
                content: format!("Completely different content {}", i),
                embedding: vec![0.1, 0.2, 0.3],
                metadata: serde_json::json!({ "dedup": true }),
                ..Default::default()
            };
            store.put(merge_rec).await.unwrap();
        }

        let fetched = store.get("test_ws", "test/path").await.unwrap().unwrap();
        assert!(
            fetched.revisions.len() <= 5,
            "Revisions must be capped at max_revisions (5), got {}",
            fetched.revisions.len()
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_put_record_without_embedding_succeeds() {
        use crate::memory::store::MemoryStore;
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        // Ensure embedding client is not configured
        std::env::remove_var("XAVIER_EMBEDDING_PROVIDER_MODE");
        std::env::remove_var("OPENAI_API_KEY");

        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_warn.db");
        let config = crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
            path: db_path,
            embedding_dimensions: 3,
        };

        let store = VecSqliteMemoryStore::new(config).await.unwrap();

        let record = crate::memory::store::MemoryRecord {
            id: "test_warn_mem_1".to_string(),
            workspace_id: "test_ws_1".to_string(),
            path: "test/path".to_string(),
            content: "Hello world without embedding".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            ..Default::default()
        };

        let res = store.put(record).await;
        assert!(res.is_ok());

        // Verify the record is saved without embedding
        let fetched = store
            .get("test_ws_1", "test_warn_mem_1")
            .await
            .unwrap()
            .unwrap();
        assert!(fetched.embedding.is_empty());
    }
}
