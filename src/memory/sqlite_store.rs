//! SQLite backend for Xavier memory store.
//!
//! Provides a persistent, ACID-compliant storage layer using SQLite.

use std::{any::Any, path::PathBuf, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::params;
use tokio::fs;

use crate::checkpoint::Checkpoint;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::connection_provider::{ConnectionProvider, GlobalConnectionProvider};
use crate::memory::schema::{MemoryLevel, MemoryQueryFilters};
use crate::memory::store::{
    filter_records, revisioned_record, stable_key, DurableWorkspaceState, MemoryBackend,
    MemoryRecord, MemoryStore, SessionTokenRecord,
};
use crate::settings::XavierSettings;

const DB_FILENAME: &str = "xavier_memory.db";
pub(crate) const TABLE_MEMORIES: &str = "memory_records";
pub(crate) const TABLE_BELIEFS: &str = "belief_states";
pub(crate) const TABLE_SESSION_TOKENS: &str = "session_tokens";
pub(crate) const TABLE_CHECKPOINTS: &str = "checkpoint_records";
pub(crate) const TABLE_PANEL_BOOKMARKS: &str = "panel_bookmarks";
pub(crate) const TABLE_PANEL_WIDGETS: &str = "panel_widgets";
pub(crate) const TABLE_PANEL_GRAPHS: &str = "panel_graphs";
pub(crate) const TABLE_NOTIFICATIONS: &str = "notifications";

struct SessionTokenRow {
    token: String,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<SessionTokenRow> for SessionTokenRecord {
    fn from(value: SessionTokenRow) -> Self {
        Self {
            token: value.token,
            created_at: value.created_at,
            expires_at: value.expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SqliteStoreConfig {
    pub path: PathBuf,
}

impl SqliteStoreConfig {
    /// From env.
    pub fn from_env() -> Self {
        let settings = XavierSettings::current();
        Self {
            path: std::env::var("XAVIER_MEMORY_SQLITE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    if settings.memory.sqlite_path.trim().is_empty() {
                        PathBuf::from(&settings.memory.data_dir).join(DB_FILENAME)
                    } else {
                        PathBuf::from(&settings.memory.sqlite_path)
                    }
                }),
        }
    }

    fn detail(&self) -> String {
        self.path.display().to_string()
    }
}

#[derive(Clone)]
pub struct SqliteMemoryStore {
    config: SqliteStoreConfig,
    project_id: String,
    conn_provider: Arc<dyn ConnectionProvider>,
}

impl SqliteMemoryStore {
    /// From env.
    pub async fn from_env() -> Result<Self> {
        Self::new(SqliteStoreConfig::from_env()).await
    }

    /// New with default global provider.
    pub async fn new(config: SqliteStoreConfig) -> Result<Self> {
        Self::new_with_provider(config, Arc::new(GlobalConnectionProvider::new())).await
    }

    /// New with isolated in-memory connection provider for test isolation.
    pub async fn new_in_memory(config: SqliteStoreConfig) -> Result<Self> {
        Self::new_with_provider(
            config,
            Arc::new(crate::memory::connection_provider::InMemoryProvider::new()),
        )
        .await
    }

    /// New with injectable connection provider.
    pub async fn new_with_provider(
        config: SqliteStoreConfig,
        conn_provider: Arc<dyn ConnectionProvider>,
    ) -> Result<Self> {
        if let Some(parent) = config.path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let project_id = format!("memory_{}", ulid::Ulid::new().to_string());
        conn_provider.connect_with_path(&project_id, config.path.clone())?;

        let store = Self {
            config,
            project_id,
            conn_provider: conn_provider.clone(),
        };

        // Initialize schema via migration manager
        let project_id_c = store.project_id.clone();
        conn_provider
            .with_conn(&project_id_c, move |conn| {
                let mut manager = crate::storage::MigrationManager::new();
                manager.add_migration(crate::storage::migrations::MigrationV1InitialSchema);
                manager.add_migration(crate::storage::migrations::MigrationV2ColumnarIndices);
                manager.add_migration(crate::storage::migrations::MigrationV3UnifiedExtensions);
                manager.add_migration(crate::storage::migrations::MigrationV4UnifiedIsolation);
                manager.add_migration(crate::storage::migrations::MigrationV5SessionTokensId);
                manager.add_migration(crate::storage::migrations::MigrationV8EntityGraphSnapshots);
                manager.add_migration(
                    crate::storage::migrations::MigrationV11CleanupMemorySymbolLinks,
                );
                manager.run_migrations(conn)
            })
            .await?;

        Ok(store)
    }

    fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
        embedding.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn deserialize_embedding(data: &[u8]) -> Vec<f32> {
        data.as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect()
    }

    fn row_key(workspace_id: &str, memory_id: &str) -> String {
        stable_key("sqlite_mem", &[workspace_id, memory_id])
    }

    fn deserialize_record(row: &rusqlite::Row) -> rusqlite::Result<MemoryRecord> {
        let metadata_str: String = row.get(4)?;
        let embedding_blob: Vec<u8> = row.get(5)?;

        // Note: Decryption is handled in MemoryStore implementation if needed.
        // We store encrypted content directly in MemoryRecord during raw fetch.
        Ok(MemoryRecord {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            path: row.get(2)?,
            content: row.get(3)?,
            metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
            embedding: Self::deserialize_embedding(&embedding_blob),
            created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            revision: row.get(8)?,
            primary: row.get::<_, i32>(9)? != 0,
            parent_id: row.get(10)?,
            cluster_id: row.get(11)?,
            level: MemoryLevel::parse(&row.get::<_, String>(12)?),
            relation: row
                .get::<_, Option<String>>(13)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            clearance: serde_json::from_str::<serde_json::Value>(&metadata_str)
                .ok()
                .and_then(|v| v.get("clearance").cloned())
                .and_then(|v| {
                    v.as_str()
                        .map(crate::security::clearance::ClearanceLevel::from)
                })
                .unwrap_or_default(),
            revisions: row
                .get::<_, Option<String>>(14)?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            encrypted_dek: row.get(15)?,
            content_iv: row.get(16)?,
            metadata_iv: row.get(17)?,
            embedding_status: row
                .get::<_, Option<String>>(18)
                .ok()
                .flatten()
                .unwrap_or_else(|| "pending".to_string()),
            embedding_attempts: row.get::<_, Option<u32>>(19).ok().flatten().unwrap_or(0),
            score: 0.0,
            deleted_at: None,
        })
    }
}

impl SqliteMemoryStore {
    /// Decrypt record.
    pub(crate) fn decrypt_record(record: &mut MemoryRecord) -> Result<()> {
        if let Some(encrypted_dek) = &record.encrypted_dek {
            let security = crate::security::get_security_service();
            let mgr = security.get_key_manager()?;
            let kek = security.get_kek()?;

            let dek = mgr
                .decrypt_dek(encrypted_dek, &kek)
                .map_err(|e| anyhow::anyhow!("DEK decryption failed: {}", e))?;

            if let Some(content_iv) = &record.content_iv {
                let ciphertext = crate::utils::crypto::hex_decode(&record.content)?;
                let nonce: [u8; 12] = content_iv
                    .as_slice()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("Invalid content IV"))?;
                let plaintext =
                    crate::crypto::encryption::decrypt_data(&ciphertext, dek.as_bytes(), &nonce)
                        .map_err(|e| anyhow::anyhow!("Content decryption failed: {}", e))?;
                record.content = String::from_utf8(plaintext)?;
            }

            if let Some(metadata_iv) = &record.metadata_iv {
                if let Some(encrypted_metadata_hex) =
                    record.metadata.get("encrypted").and_then(|v| v.as_str())
                {
                    let ciphertext = crate::utils::crypto::hex_decode(encrypted_metadata_hex)?;
                    let nonce: [u8; 12] = metadata_iv
                        .as_slice()
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("Invalid metadata IV"))?;
                    let plaintext = crate::crypto::encryption::decrypt_data(
                        &ciphertext,
                        dek.as_bytes(),
                        &nonce,
                    )
                    .map_err(|e| anyhow::anyhow!("Metadata decryption failed: {}", e))?;
                    record.metadata = serde_json::from_slice(&plaintext)?;
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Sqlite
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn compact(&self) -> Result<()> {
        let project_id = self.project_id.clone();
        self.conn_provider
            .with_conn(&project_id, move |conn| {
                conn.execute_batch("VACUUM;")?;
                Ok(())
            })
            .await
    }

    async fn db_size(&self) -> Result<Option<u64>> {
        if self.config.path.exists() {
            if let Ok(metadata) = std::fs::metadata(&self.config.path) {
                return Ok(Some(metadata.len()));
            }
        }
        Ok(None)
    }

    async fn health(&self) -> Result<String> {
        let detail = self.config.detail();
        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                conn.query_row("SELECT 1", [], |_row| Ok(()))?;
                Ok(format!("sqlite {}", detail))
            })
            .await
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let mut record = record;
        let security = crate::security::get_security_service();
        if security.get_config().encryption_at_rest_enabled {
            let mgr = security.get_key_manager()?;
            let kek = security.get_kek()?;

            // Get or create salt for this workspace
            let workspace_id = record.workspace_id.clone();
            let project_id = self.project_id.clone();
            let _salt_bytes = self.conn_provider
                .with_conn(&project_id, move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT salt FROM encryption_metadata WHERE workspace_id = ?",
                    )?;
                    match stmt.query_row([&workspace_id], |row| row.get::<_, Vec<u8>>(0)) {
                        Ok(salt) => Ok(salt),
                        Err(rusqlite::Error::QueryReturnedNoRows) => {
                            let new_salt = crate::crypto::keys::KeySalt::generate();
                            let salt_vec = new_salt.as_bytes().to_vec();
                            conn.execute(
                                "INSERT INTO encryption_metadata (id, workspace_id, salt, created_at) VALUES (?, ?, ?, ?)",
                                params![ulid::Ulid::new().to_string(), workspace_id, salt_vec, Utc::now().to_rfc3339()],
                            )?;
                            Ok(salt_vec)
                        }
                        Err(e) => Err(anyhow::anyhow!("Database error: {}", e)),
                    }
                })
                .await?;

            // Generate DEK
            let dek = mgr.generate_dek();
            let encrypted_dek = mgr
                .encrypt_dek(&dek, &kek)
                .map_err(|e| anyhow::anyhow!("DEK encryption failed: {}", e))?;

            // Encrypt content
            let content_nonce = crate::crypto::encryption::NonceBytes::generate();
            let encrypted_content = crate::crypto::encryption::encrypt_data(
                record.content.as_bytes(),
                dek.as_bytes(),
                &content_nonce,
            )
            .map_err(|e| anyhow::anyhow!("Content encryption failed: {}", e))?;

            // Encrypt metadata
            let metadata_nonce = crate::crypto::encryption::NonceBytes::generate();
            let metadata_json = serde_json::to_string(&record.metadata)?;
            let encrypted_metadata = crate::crypto::encryption::encrypt_data(
                metadata_json.as_bytes(),
                dek.as_bytes(),
                &metadata_nonce,
            )
            .map_err(|e| anyhow::anyhow!("Metadata encryption failed: {}", e))?;

            record.content = crate::utils::crypto::hex_encode(&encrypted_content.ciphertext);
            record.metadata = serde_json::json!({
                "encrypted": crate::utils::crypto::hex_encode(&encrypted_metadata.ciphertext)
            });
            record.encrypted_dek = Some(encrypted_dek);
            record.content_iv = Some(content_nonce.as_bytes().to_vec());
            record.metadata_iv = Some(metadata_nonce.as_bytes().to_vec());
        }

        self.conn_provider.with_conn(&self.project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, path, content, metadata, embedding, encrypted_dek, content_iv, metadata_iv, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                    TABLE_MEMORIES
                ),
                params![
                    record.id,
                    record.workspace_id,
                    record.path,
                    record.content,
                    serde_json::to_string(&record.metadata).unwrap_or_default(),
                    Self::serialize_embedding(&record.embedding),
                    record.encrypted_dek,
                    record.content_iv,
                    record.metadata_iv,
                    record.created_at.to_rfc3339(),
                    record.updated_at.to_rfc3339(),
                    record.revision,
                    record.primary as i32,
                    record.parent_id,
                    record.cluster_id,
                    record.level.as_str(),
                    serde_json::to_string(&record.relation).unwrap_or_default(),
                    serde_json::to_string(&record.revisions).unwrap_or_default(),
                ],
            )?;
            Ok(())
        }).await
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let workspace_id = workspace_id.to_string();
        let id_or_path = id_or_path.to_string();

        let record = self.conn_provider.with_conn(&self.project_id, move |conn| {
            // Try by id first (O(1) lookup)
            let key = Self::row_key(&workspace_id, &id_or_path);
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE id = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query([&key])?;
            if let Some(row) = rows.next()? {
                return Ok(Some(Self::deserialize_record(row)?));
            }
            drop(rows);
            drop(stmt);

            // Fallback: try by path
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE workspace_id = ? AND path = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query(params![workspace_id, id_or_path])?;
            if let Some(row) = rows.next()? {
                Ok(Some(Self::deserialize_record(row)?))
            } else {
                Ok(None)
            }
        }).await?;

        if let Some(mut record) = record {
            Self::decrypt_record(&mut record)?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        let record = if let Some(existing) = self.get(&record.workspace_id, &record.id).await? {
            revisioned_record(existing, record)
        } else if let Some(existing) = self.get(&record.workspace_id, &record.path).await? {
            revisioned_record(existing, record)
        } else {
            record
        };
        self.put(record).await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let removed = self.get(workspace_id, id_or_path).await?;
        if let Some(record) = &removed {
            let key = Self::row_key(workspace_id, &record.id);
            let workspace_id = workspace_id.to_string();
            let record_id = record.id.clone();

            self.conn_provider
                .with_conn(&self.project_id, move |conn| {
                    conn.execute(
                        &format!("DELETE FROM {} WHERE id = ?", TABLE_MEMORIES),
                        [&key],
                    )?;

                    // Also delete children
                    conn.execute(
                        &format!(
                            "DELETE FROM {} WHERE workspace_id = ? AND parent_id = ?",
                            TABLE_MEMORIES
                        ),
                        params![workspace_id, record_id],
                    )?;
                    Ok(())
                })
                .await?;
        }
        Ok(removed)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let workspace_id = workspace_id.to_string();
        let records = self.conn_provider.with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE workspace_id = ?",
                TABLE_MEMORIES
            ))?;

            let mut rows = stmt.query([workspace_id])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                if let Ok(record) = Self::deserialize_record(row) {
                    records.push(record);
                }
            }
            Ok(records)
        }).await?;

        let mut results = Vec::with_capacity(records.len());
        for mut record in records {
            Self::decrypt_record(&mut record)?;
            results.push(record);
        }
        Ok(results)
    }

    async fn list_filtered(
        &self,
        workspace_id: &str,
        filters: &MemoryQueryFilters,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>> {
        let all = self.list(workspace_id).await?;
        Ok(filter_records(all, workspace_id, "", Some(filters))?
            .into_iter()
            .take(limit)
            .collect())
    }

    async fn list_workspaces(&self) -> Result<Vec<String>> {
        let records = self
            .conn_provider
            .with_conn(&self.project_id, |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT DISTINCT workspace_id FROM {}",
                    TABLE_MEMORIES
                ))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                let mut ids = Vec::new();
                for row in rows {
                    ids.push(row?);
                }
                Ok(ids)
            })
            .await?;
        Ok(records)
    }

    async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        filters: Option<&MemoryQueryFilters>,
    ) -> Result<Vec<MemoryRecord>> {
        let records = self.list(workspace_id).await?;
        filter_records(records, workspace_id, query, filters)
    }

    async fn load_workspace_state(&self, workspace_id: &str) -> Result<DurableWorkspaceState> {
        let workspace_id_c = workspace_id.to_string();

        self.conn_provider.with_conn(&self.project_id, move |conn| {
            // Load memories
            let mut memories = Vec::new();
            {
                let mut stmt = conn.prepare(&format!(
                    "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at, revision, primary_flag, parent_id, cluster_id, level, relation, revisions, encrypted_dek, content_iv, metadata_iv FROM {} WHERE workspace_id = ?",
                    TABLE_MEMORIES
                ))?;
                let mut rows = stmt.query([&workspace_id_c])?;
                while let Some(row) = rows.next()? {
                    if let Ok(record) = Self::deserialize_record(row) {
                        memories.push(record);
                    }
                }
            }

            // Load beliefs
            let beliefs = {
                let belief_key = stable_key("belief_row", &[&workspace_id_c]);
                let mut stmt = conn.prepare(&format!(
                    "SELECT beliefs FROM {} WHERE id = ?",
                    TABLE_BELIEFS
                ))?;
                stmt.query_row([&belief_key], |row| {
                    let val = row.get_ref(0)?;
                    match val {
                        rusqlite::types::ValueRef::Text(b) => {
                            let s = std::str::from_utf8(b).unwrap_or("");
                            Ok(serde_json::from_str(s).unwrap_or_default())
                        }
                        rusqlite::types::ValueRef::Blob(b) => {
                            Ok(crate::utils::compression::decompress_payload(b).unwrap_or_default())
                        }
                        _ => Ok(Vec::new()),
                    }
                })
                .unwrap_or_default()
            };

            // Load session tokens (filter expired)
            let now = Utc::now();
            let session_tokens = {
                let mut stmt = conn.prepare(&format!(
                    "SELECT id, workspace_id, token, created_at, expires_at FROM {} WHERE workspace_id = ?",
                    TABLE_SESSION_TOKENS
                ))?;
                let mut rows = stmt.query([&workspace_id_c])?;
                let mut tokens = Vec::new();
                while let Some(row) = rows.next()? {
                    let token_row = SessionTokenRow {
                        token: row.get(2)?,
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        expires_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    };
                    if token_row.expires_at > now {
                        tokens.push(SessionTokenRecord::from(token_row));
                    }
                }
                tokens
            };

            // Load checkpoints
            let checkpoints = {
                let mut stmt = conn.prepare(&format!(
                    "SELECT task_id, name, data FROM {} WHERE workspace_id = ?",
                    TABLE_CHECKPOINTS
                ))?;
                let mut rows = stmt.query([&workspace_id_c])?;
                let mut checkpoints = Vec::new();
                while let Some(row) = rows.next()? {
                    let data = match row.get_ref(2)? {
                        rusqlite::types::ValueRef::Text(b) => {
                            let s = std::str::from_utf8(b).unwrap_or("");
                            serde_json::from_str(s).unwrap_or_default()
                        }
                        rusqlite::types::ValueRef::Blob(b) => {
                            crate::utils::compression::decompress_payload(b).unwrap_or_default()
                        }
                        _ => serde_json::Value::Null,
                    };
                    checkpoints.push(Checkpoint {
                        task_id: row.get(0)?,
                        name: row.get(1)?,
                        data,
                    });
                }
                checkpoints
            };

            Ok(DurableWorkspaceState {
                memories,
                beliefs,
                session_tokens,
                checkpoints,
                entity_graph_snapshot: None,
            })
        }).await
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let belief_key = stable_key("belief_row", &[&workspace_id]);
        let beliefs_blob = crate::utils::compression::compress_payload(&beliefs)?;

        self.conn_provider.with_conn(&self.project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, beliefs, updated_at) VALUES (?, ?, ?, ?)",
                    TABLE_BELIEFS
                ),
                params![belief_key, workspace_id, beliefs_blob, Utc::now().to_rfc3339()],
            )?;
            Ok(())
        }).await
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let token_key = stable_key("session_token_row", &[&workspace_id, &token.token]);
        let token_val = token.token;
        let created_at = token.created_at.to_rfc3339();
        let expires_at = token.expires_at.to_rfc3339();

        self.conn_provider.with_conn(&self.project_id, move |conn| {
            // Delete expired tokens first
            conn.execute(
                &format!(
                    "DELETE FROM {} WHERE workspace_id = ? AND expires_at <= ?",
                    TABLE_SESSION_TOKENS
                ),
                params![workspace_id, Utc::now().to_rfc3339()],
            )?;

            // Insert new token
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, token, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
                    TABLE_SESSION_TOKENS
                ),
                params![
                    token_key,
                    workspace_id,
                    token_val,
                    created_at,
                    expires_at,
                ],
            )?;
            Ok(())
        }).await
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let workspace_id = workspace_id.to_string();
        let token = token.to_string();

        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                let token_key = stable_key("session_token_row", &[&workspace_id, &token]);
                let now = Utc::now().to_rfc3339();

                let count: i32 = conn.query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {} WHERE id = ? AND expires_at > ?",
                        TABLE_SESSION_TOKENS
                    ),
                    params![token_key, now],
                    |row| row.get(0),
                )?;

                Ok(count > 0)
            })
            .await
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let checkpoint_key = stable_key(
            "checkpoint_row",
            &[&workspace_id, &checkpoint.task_id, &checkpoint.name],
        );
        let task_id = checkpoint.task_id;
        let name = checkpoint.name;
        let data_blob = crate::utils::compression::compress_payload(&checkpoint.data)?;

        self.conn_provider.with_conn(&self.project_id, move |conn| {
            conn.execute(
                &format!(
                    "INSERT OR REPLACE INTO {} (id, workspace_id, task_id, name, data) VALUES (?, ?, ?, ?, ?)",
                    TABLE_CHECKPOINTS
                ),
                params![checkpoint_key, workspace_id, task_id, name, data_blob],
            )?;
            Ok(())
        }).await
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let workspace_id = workspace_id.to_string();
        let task_id = task_id.to_string();
        let name = name.to_string();

        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT data FROM {} WHERE workspace_id = ? AND task_id = ? AND name = ?",
                    TABLE_CHECKPOINTS
                ))?;

                match stmt.query_row(params![workspace_id, task_id, name], |row| {
                    let val = row.get_ref(0)?;
                    match val {
                        rusqlite::types::ValueRef::Text(b) => {
                            let s = std::str::from_utf8(b).unwrap_or("");
                            Ok(serde_json::from_str(s).unwrap_or_default())
                        }
                        rusqlite::types::ValueRef::Blob(b) => Ok(
                            crate::utils::compression::decompress_payload(b).unwrap_or_default(),
                        ),
                        _ => Ok(serde_json::Value::Null),
                    }
                }) {
                    Ok(data) => Ok(Some(Checkpoint {
                        task_id,
                        name,
                        data,
                    })),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(anyhow::anyhow!("SQLite query failed: {}", e)),
                }
            })
            .await
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let workspace_id = workspace_id.to_string();
        let task_id = task_id.to_string();

        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT task_id, name, data FROM {} WHERE workspace_id = ? AND task_id = ?",
                    TABLE_CHECKPOINTS
                ))?;

                let mut rows = stmt.query(params![workspace_id, task_id])?;
                let mut checkpoints = Vec::new();
                while let Some(row) = rows.next()? {
                    let data = match row.get_ref(2)? {
                        rusqlite::types::ValueRef::Text(b) => {
                            let s = std::str::from_utf8(b).unwrap_or("");
                            serde_json::from_str(s).unwrap_or_default()
                        }
                        rusqlite::types::ValueRef::Blob(b) => {
                            crate::utils::compression::decompress_payload(b).unwrap_or_default()
                        }
                        _ => serde_json::Value::Null,
                    };
                    checkpoints.push(Checkpoint {
                        task_id: row.get(0)?,
                        name: row.get(1)?,
                        data,
                    });
                }
                Ok(checkpoints)
            })
            .await
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let task_id = task_id.to_string();
        let name = name.to_string();

        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                let checkpoint_key =
                    stable_key("checkpoint_row", &[&workspace_id, &task_id, &name]);
                conn.execute(
                    &format!("DELETE FROM {} WHERE id = ?", TABLE_CHECKPOINTS),
                    [&checkpoint_key],
                )?;
                Ok(())
            })
            .await
    }

    async fn load_entity_graph_snapshot(&self, workspace_id: &str) -> Result<Option<String>> {
        let workspace_id = workspace_id.to_string();
        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                let mut stmt =
                    conn.prepare("SELECT data FROM entity_graph_snapshots WHERE workspace_id = ?")?;
                match stmt.query_row([&workspace_id], |row| row.get::<_, String>(0)) {
                    Ok(data) => Ok(Some(data)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(anyhow::anyhow!("SQLite query failed: {}", e)),
                }
            })
            .await
    }

    async fn save_entity_graph_snapshot(&self, workspace_id: &str, data: &str) -> Result<()> {
        let workspace_id = workspace_id.to_string();
        let data = data.to_string();
        let now = Utc::now().to_rfc3339();
        self.conn_provider
            .with_conn(&self.project_id, move |conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO entity_graph_snapshots (workspace_id, data, updated_at) VALUES (?, ?, ?)",
                    params![workspace_id, data, now],
                )?;
                Ok(())
            })
            .await
    }
}

impl SqliteMemoryStore {
    /// Migrate legacy flat paths to hierarchy paths (prefix with `src/` if path has no slash)
    /// and backfill parent directory paths in the store.
    pub async fn migrate_paths_hierarchy(&self) -> Result<usize> {
        let project_id = self.project_id.clone();
        self.conn_provider
            .with_conn(&project_id, move |conn| {
                let tx = conn.unchecked_transaction()?;

                let updated_count = tx.execute(
                    &format!(
                        "UPDATE {} SET path = 'src/' || path WHERE path NOT LIKE '%/%' AND path != ''",
                        TABLE_MEMORIES
                    ),
                    [],
                )?;

                tx.commit()?;
                Ok(updated_count)
            })
            .await
    }

    /// List distinct memory record paths matching a given hierarchy prefix.
    pub async fn list_hierarchy(&self, prefix: &str) -> Result<Vec<String>> {
        let prefix = prefix.to_string();
        let project_id = self.project_id.clone();

        self.conn_provider
            .with_conn(&project_id, move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT DISTINCT path FROM {} WHERE path LIKE ? || '%' ORDER BY path ASC",
                    TABLE_MEMORIES
                ))?;

                let rows = stmt.query_map([&prefix], |row| row.get::<_, String>(0))?;
                let mut paths = Vec::new();
                for row in rows {
                    paths.push(row?);
                }
                Ok(paths)
            })
            .await
    }

    /// Retrieve path hierarchy statistics, including total memory record count
    /// and distribution counts by directory prefix.
    pub async fn get_hierarchy_stats(&self) -> Result<serde_json::Value> {
        let project_id = self.project_id.clone();

        self.conn_provider
            .with_conn(&project_id, move |conn| {
                let total: usize = conn.query_row(
                    &format!("SELECT COUNT(*) FROM {}", TABLE_MEMORIES),
                    [],
                    |row| row.get(0),
                )?;

                let mut stmt = conn.prepare(&format!(
                    "SELECT path FROM {} WHERE path LIKE '%/%'",
                    TABLE_MEMORIES
                ))?;

                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                let mut prefix_counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();

                for path_res in rows {
                    let path = path_res?;
                    let parts: Vec<&str> = path.split('/').collect();
                    if parts.len() > 1 {
                        let mut current_prefix = String::new();
                        for dir in &parts[..parts.len() - 1] {
                            current_prefix.push_str(dir);
                            current_prefix.push('/');
                            *prefix_counts.entry(current_prefix.clone()).or_insert(0) += 1;
                        }
                    }
                }

                let mut by_prefix_map = serde_json::Map::new();
                for (k, v) in prefix_counts {
                    by_prefix_map.insert(k, serde_json::Value::Number(v.into()));
                }

                Ok(serde_json::json!({
                    "total": total,
                    "by_prefix": by_prefix_map,
                }))
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_path_hierarchy_preserved() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_hierarchy.db");
        let config = SqliteStoreConfig { path: db_path };
        let store = SqliteMemoryStore::new_in_memory(config).await.unwrap();

        let rec1 = MemoryRecord {
            id: "rec1".to_string(),
            workspace_id: "ws_test".to_string(),
            path: "src/agents/foo.rs".to_string(),
            content: "agent foo content".to_string(),
            ..Default::default()
        };
        let rec2 = MemoryRecord {
            id: "rec2".to_string(),
            workspace_id: "ws_test".to_string(),
            path: "src/agents/bar.rs".to_string(),
            content: "agent bar content".to_string(),
            ..Default::default()
        };
        let rec3 = MemoryRecord {
            id: "rec3".to_string(),
            workspace_id: "ws_test".to_string(),
            path: "src/memory/store.rs".to_string(),
            content: "memory store content".to_string(),
            ..Default::default()
        };

        store.put(rec1).await.unwrap();
        store.put(rec2).await.unwrap();
        store.put(rec3).await.unwrap();

        let agent_paths = store.list_hierarchy("src/agents/").await.unwrap();
        assert_eq!(agent_paths.len(), 2);
        assert_eq!(agent_paths[0], "src/agents/bar.rs");
        assert_eq!(agent_paths[1], "src/agents/foo.rs");

        let src_paths = store.list_hierarchy("src/").await.unwrap();
        assert_eq!(src_paths.len(), 3);

        let stats = store.get_hierarchy_stats().await.unwrap();
        assert_eq!(stats["total"], 3);
        assert_eq!(stats["by_prefix"]["src/"], 3);
        assert_eq!(stats["by_prefix"]["src/agents/"], 2);
        assert_eq!(stats["by_prefix"]["src/memory/"], 1);
    }

    #[tokio::test]
    async fn test_migrate_paths_hierarchy() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test_migrate.db");
        let config = SqliteStoreConfig { path: db_path };
        let store = SqliteMemoryStore::new_in_memory(config).await.unwrap();

        let flat_rec = MemoryRecord {
            id: "flat1".to_string(),
            workspace_id: "ws_test".to_string(),
            path: "foo.rs".to_string(),
            content: "flat path content".to_string(),
            ..Default::default()
        };
        let nested_rec = MemoryRecord {
            id: "nested1".to_string(),
            workspace_id: "ws_test".to_string(),
            path: "src/bar.rs".to_string(),
            content: "nested path content".to_string(),
            ..Default::default()
        };

        store.put(flat_rec).await.unwrap();
        store.put(nested_rec).await.unwrap();

        let updated = store.migrate_paths_hierarchy().await.unwrap();
        assert_eq!(updated, 1);

        let fetched_flat = store.get("ws_test", "src/foo.rs").await.unwrap().unwrap();
        assert_eq!(fetched_flat.id, "flat1");
        assert_eq!(fetched_flat.path, "src/foo.rs");

        let fetched_nested = store.get("ws_test", "src/bar.rs").await.unwrap().unwrap();
        assert_eq!(fetched_nested.id, "nested1");
        assert_eq!(fetched_nested.path, "src/bar.rs");
    }
}
