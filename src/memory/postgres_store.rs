//! PostgreSQL backend for Xavier memory store (supports Neon/pgvector).

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres, Row};
use std::any::Any;

use crate::checkpoint::Checkpoint;
use crate::domain::memory::belief::BeliefEdge;
use crate::memory::schema::{MemoryLevel, MemoryQueryFilters};
use crate::memory::store::{
    filter_records, stable_key, DurableWorkspaceState, MemoryBackend, MemoryRecord, MemoryStore,
    SessionTokenRecord,
};
use crate::settings::XavierSettings;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn parse_vector_text(s: &str) -> Vec<f32> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let inner = trimmed
        .strip_prefix('[')
        .unwrap_or(trimmed)
        .strip_suffix(']')
        .unwrap_or(trimmed)
        .trim();
    if inner.is_empty() {
        return Vec::new();
    }
    inner
        .split(',')
        .filter_map(|part| part.trim().parse::<f32>().ok())
        .collect()
}

pub fn parse_vector_binary(bytes: &[u8]) -> Vec<f32> {
    if bytes.is_empty() {
        return Vec::new();
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        if text.trim().starts_with('[') || text.contains(',') {
            return parse_vector_text(text);
        }
    }

    if bytes.len() >= 4 {
        let dim = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
        let expected_len = 4 + dim * 4;
        if dim > 0 && bytes.len() == expected_len {
            let mut result = Vec::with_capacity(dim);
            for chunk in bytes[4..].as_chunks::<4>().0 {
                let val = f32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                result.push(val);
            }
            return result;
        }
    }

    if bytes.len().is_multiple_of(4) {
        let mut result = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.as_chunks::<4>().0 {
            let val = f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            result.push(val);
        }
        return result;
    }

    Vec::new()
}

fn parse_vector_from_row(row: &sqlx::postgres::PgRow) -> Vec<f32> {
    if let Ok(s) = row.try_get::<String, _>("embedding") {
        return parse_vector_text(&s);
    }
    if let Ok(bytes) = row.try_get::<Vec<u8>, _>("embedding") {
        return parse_vector_binary(&bytes);
    }
    Vec::new()
}

pub fn shard_for_id(id: &str) -> u8 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 2) as u8
}

#[derive(Clone)]
pub struct PostgresMemoryStore {
    pool: Pool<Postgres>,
    url: String,
    /// Secondary Neon project via XAVIER_POSTGRES_URL_2 (branch per node)
    pool_2: Option<Pool<Postgres>>,
    url_2: Option<String>,
}

impl PostgresMemoryStore {
    /// From env.
    pub async fn from_env() -> Result<Self> {
        let settings = XavierSettings::current();
        let url = std::env::var("XAVIER_POSTGRES_URL")
            .ok()
            .or_else(|| settings.memory.postgres_url.clone())
            .context("XAVIER_POSTGRES_URL or settings.memory.postgres_url not set")?;

        Self::new(&url).await
    }

    /// New.
    pub async fn new(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new().max_connections(5).connect(url).await?;

        let url_2 = std::env::var("XAVIER_POSTGRES_URL_2").ok();
        let pool_2 = if let Some(ref u2) = url_2 {
            PgPoolOptions::new()
                .max_connections(5)
                .connect(u2)
                .await
                .ok()
        } else {
            None
        };

        let store = Self {
            pool,
            url: url.to_string(),
            pool_2,
            url_2,
        };

        store.init_schema().await?;
        if let Some(ref p2) = store.pool_2 {
            // init schema on second shard as well (branch per node)
            let _ = sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
                .execute(p2)
                .await;
        }

        Ok(store)
    }

    pub fn shard_for(&self, id: &str) -> u8 {
        shard_for_id(id)
    }
    pub fn is_sharded(&self) -> bool {
        self.pool_2.is_some()
    }
    fn pool_for_shard(&self, shard: u8) -> &Pool<Postgres> {
        if shard == 1 {
            if let Some(p2) = self.pool_2.as_ref() {
                return p2;
            }
        }
        &self.pool
    }

    /// Health check.
    pub async fn health_check(&self) -> Result<()> {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            sqlx::query("SELECT 1").execute(&self.pool),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Postgres health check timed out after 10s"))??;
        Ok(())
    }

    async fn init_schema(&self) -> Result<()> {
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&self.pool)
            .await
            .ok();

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS memory_records (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata JSONB NOT NULL,
                embedding VECTOR(1536),
                created_at TIMESTAMPTZ NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL,
                revision BIGINT NOT NULL,
                primary_flag BOOLEAN NOT NULL,
                parent_id TEXT,
                cluster_id TEXT,
                level TEXT NOT NULL,
                relation JSONB,
                revisions JSONB,
                encrypted_dek BYTEA,
                content_iv BYTEA,
                metadata_iv BYTEA
            )
        "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS belief_states (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                beliefs JSONB NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL
            )
        "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS session_tokens (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                token TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL,
                expires_at TIMESTAMPTZ NOT NULL
            )
        "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS checkpoint_records (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                name TEXT NOT NULL,
                data JSONB NOT NULL
            )
        "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    fn row_to_record(row: sqlx::postgres::PgRow) -> Result<MemoryRecord> {
        let metadata: serde_json::Value = row.try_get("metadata")?;
        let relation: serde_json::Value = row.try_get("relation")?;
        let revisions: serde_json::Value = row.try_get("revisions")?;
        let embedding = parse_vector_from_row(&row);

        Ok(MemoryRecord {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            path: row.try_get("path")?,
            content: row.try_get("content")?,
            metadata,
            embedding,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            revision: row.try_get::<i64, _>("revision")? as u64,
            primary: row.try_get("primary_flag")?,
            parent_id: row.try_get("parent_id")?,
            cluster_id: row.try_get("cluster_id")?,
            level: MemoryLevel::parse(&row.try_get::<String, _>("level")?),
            relation: serde_json::from_value(relation).ok(),
            clearance: Default::default(),
            revisions: serde_json::from_value(revisions).unwrap_or_default(),
            encrypted_dek: row.try_get("encrypted_dek")?,
            content_iv: row.try_get("content_iv")?,
            metadata_iv: row.try_get("metadata_iv")?,
            score: 0.0,
            deleted_at: None,
            embedding_status: row
                .try_get("embedding_status")
                .unwrap_or_else(|_| "pending".to_string()),
            embedding_attempts: row
                .try_get::<i32, _>("embedding_attempts")
                .map(|v| v as u32)
                .unwrap_or(0),
        })
    }
}

#[async_trait]
impl MemoryStore for PostgresMemoryStore {
    fn backend(&self) -> MemoryBackend {
        MemoryBackend::Postgres
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    async fn health(&self) -> Result<String> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(format!("postgres connected to {}", self.url))
    }

    async fn put(&self, record: MemoryRecord) -> Result<()> {
        let relation = serde_json::to_value(&record.relation)?;
        let revisions = serde_json::to_value(&record.revisions)?;
        let embedding_str = if record.embedding.is_empty() {
            None
        } else {
            Some(format!(
                "[{}]",
                record
                    .embedding
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        };

        sqlx::query(
            r#"
            INSERT INTO memory_records (
                id, workspace_id, path, content, metadata, embedding, created_at, updated_at,
                revision, primary_flag, parent_id, cluster_id, level, relation,
                revisions, encrypted_dek, content_iv, metadata_iv
            ) VALUES ($1, $2, $3, $4, $5, $6::vector, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            ON CONFLICT (id) DO UPDATE SET
                workspace_id = EXCLUDED.workspace_id,
                path = EXCLUDED.path,
                content = EXCLUDED.content,
                metadata = EXCLUDED.metadata,
                embedding = EXCLUDED.embedding,
                updated_at = EXCLUDED.updated_at,
                revision = EXCLUDED.revision,
                primary_flag = EXCLUDED.primary_flag,
                parent_id = EXCLUDED.parent_id,
                cluster_id = EXCLUDED.cluster_id,
                level = EXCLUDED.level,
                relation = EXCLUDED.relation,
                revisions = EXCLUDED.revisions,
                encrypted_dek = EXCLUDED.encrypted_dek,
                content_iv = EXCLUDED.content_iv,
                metadata_iv = EXCLUDED.metadata_iv
        "#,
        )
        .bind(&record.id)
        .bind(&record.workspace_id)
        .bind(&record.path)
        .bind(&record.content)
        .bind(&record.metadata)
        .bind(embedding_str)
        .bind(record.created_at)
        .bind(record.updated_at)
        .bind(record.revision as i64)
        .bind(record.primary)
        .bind(&record.parent_id)
        .bind(&record.cluster_id)
        .bind(record.level.as_str())
        .bind(relation)
        .bind(revisions)
        .bind(&record.encrypted_dek)
        .bind(&record.content_iv)
        .bind(&record.metadata_iv)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let key = stable_key("sqlite_mem", &[workspace_id, id_or_path]);

        let row = sqlx::query(
            r#"
            SELECT * FROM memory_records WHERE id = $1 OR (workspace_id = $2 AND path = $1)
        "#,
        )
        .bind(&key)
        .bind(workspace_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(match row {
            Some(row) => Some(Self::row_to_record(row)?),
            None => None,
        })
    }

    async fn update(&self, record: MemoryRecord) -> Result<()> {
        self.put(record).await
    }

    async fn delete(&self, workspace_id: &str, id_or_path: &str) -> Result<Option<MemoryRecord>> {
        let existing = self.get(workspace_id, id_or_path).await?;
        if let Some(ref record) = existing {
            sqlx::query("DELETE FROM memory_records WHERE id = $1")
                .bind(&record.id)
                .execute(&self.pool)
                .await?;

            // Delete children
            sqlx::query("DELETE FROM memory_records WHERE workspace_id = $1 AND parent_id = $2")
                .bind(workspace_id)
                .bind(&record.id)
                .execute(&self.pool)
                .await?;
        }
        Ok(existing)
    }

    async fn list(&self, workspace_id: &str) -> Result<Vec<MemoryRecord>> {
        let rows = sqlx::query("SELECT * FROM memory_records WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_all(&self.pool)
            .await?;

        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            records.push(Self::row_to_record(row)?);
        }
        Ok(records)
    }

    async fn list_workspaces(&self) -> Result<Vec<String>> {
        let rows =
            sqlx::query_scalar::<_, String>("SELECT DISTINCT workspace_id FROM memory_records")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
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
        let memories = self.list(workspace_id).await?;

        let belief_key = stable_key("belief_row", &[workspace_id]);
        let beliefs_row = sqlx::query("SELECT beliefs FROM belief_states WHERE id = $1")
            .bind(&belief_key)
            .fetch_optional(&self.pool)
            .await?;

        let beliefs = match beliefs_row {
            Some(row) => serde_json::from_value(row.try_get("beliefs")?).unwrap_or_default(),
            None => Vec::new(),
        };

        let now = Utc::now();
        let tokens_rows =
            sqlx::query("SELECT * FROM session_tokens WHERE workspace_id = $1 AND expires_at > $2")
                .bind(workspace_id)
                .bind(now)
                .fetch_all(&self.pool)
                .await?;

        let mut session_tokens = Vec::new();
        for row in tokens_rows {
            session_tokens.push(SessionTokenRecord {
                token: row.try_get("token")?,
                created_at: row.try_get("created_at")?,
                expires_at: row.try_get("expires_at")?,
            });
        }

        let checkpoints_rows =
            sqlx::query("SELECT * FROM checkpoint_records WHERE workspace_id = $1")
                .bind(workspace_id)
                .fetch_all(&self.pool)
                .await?;

        let mut checkpoints = Vec::new();
        for row in checkpoints_rows {
            checkpoints.push(Checkpoint {
                task_id: row.try_get("task_id")?,
                name: row.try_get("name")?,
                data: row.try_get("data")?,
            });
        }

        Ok(DurableWorkspaceState {
            memories,
            beliefs,
            session_tokens,
            checkpoints,
            entity_graph_snapshot: None,
        })
    }

    async fn save_beliefs(&self, workspace_id: &str, beliefs: Vec<BeliefEdge>) -> Result<()> {
        let belief_key = stable_key("belief_row", &[workspace_id]);
        let beliefs_json = serde_json::to_value(&beliefs)?;

        sqlx::query(
            r#"
            INSERT INTO belief_states (id, workspace_id, beliefs, updated_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (id) DO UPDATE SET
                beliefs = EXCLUDED.beliefs,
                updated_at = EXCLUDED.updated_at
        "#,
        )
        .bind(&belief_key)
        .bind(workspace_id)
        .bind(beliefs_json)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn save_session_token(
        &self,
        workspace_id: &str,
        token: SessionTokenRecord,
    ) -> Result<()> {
        let token_key = stable_key("session_token_row", &[workspace_id, &token.token]);

        sqlx::query(
            r#"
            INSERT INTO session_tokens (id, workspace_id, token, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                token = EXCLUDED.token,
                expires_at = EXCLUDED.expires_at
        "#,
        )
        .bind(&token_key)
        .bind(workspace_id)
        .bind(&token.token)
        .bind(token.created_at)
        .bind(token.expires_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn is_session_token_valid(&self, workspace_id: &str, token: &str) -> Result<bool> {
        let token_key = stable_key("session_token_row", &[workspace_id, token]);
        let now = Utc::now();

        let row = sqlx::query("SELECT 1 FROM session_tokens WHERE id = $1 AND expires_at > $2")
            .bind(&token_key)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.is_some())
    }

    async fn save_checkpoint(&self, workspace_id: &str, checkpoint: Checkpoint) -> Result<()> {
        let checkpoint_key = stable_key(
            "checkpoint_row",
            &[workspace_id, &checkpoint.task_id, &checkpoint.name],
        );

        sqlx::query(
            r#"
            INSERT INTO checkpoint_records (id, workspace_id, task_id, name, data)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (id) DO UPDATE SET
                data = EXCLUDED.data
        "#,
        )
        .bind(&checkpoint_key)
        .bind(workspace_id)
        .bind(&checkpoint.task_id)
        .bind(&checkpoint.name)
        .bind(&checkpoint.data)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn load_checkpoint(
        &self,
        workspace_id: &str,
        task_id: &str,
        name: &str,
    ) -> Result<Option<Checkpoint>> {
        let row = sqlx::query("SELECT * FROM checkpoint_records WHERE workspace_id = $1 AND task_id = $2 AND name = $3")
            .bind(workspace_id)
            .bind(task_id)
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;

        Ok(match row {
            Some(row) => Some(Checkpoint {
                task_id: row.try_get("task_id")?,
                name: row.try_get("name")?,
                data: row.try_get("data")?,
            }),
            None => None,
        })
    }

    async fn list_checkpoints(&self, workspace_id: &str, task_id: &str) -> Result<Vec<Checkpoint>> {
        let rows = sqlx::query(
            "SELECT * FROM checkpoint_records WHERE workspace_id = $1 AND task_id = $2",
        )
        .bind(workspace_id)
        .bind(task_id)
        .fetch_all(&self.pool)
        .await?;

        let mut checkpoints = Vec::with_capacity(rows.len());
        for row in rows {
            checkpoints.push(Checkpoint {
                task_id: row.try_get("task_id")?,
                name: row.try_get("name")?,
                data: row.try_get("data")?,
            });
        }
        Ok(checkpoints)
    }

    async fn delete_checkpoint(&self, workspace_id: &str, task_id: &str, name: &str) -> Result<()> {
        sqlx::query(
            "DELETE FROM checkpoint_records WHERE workspace_id = $1 AND task_id = $2 AND name = $3",
        )
        .bind(workspace_id)
        .bind(task_id)
        .bind(name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod shard_tests {
    use super::*;

    #[test]
    fn test_shard_for_id_pg() {
        assert!(shard_for_id("a") <= 1);
        assert_eq!(shard_for_id("x"), shard_for_id("x"));
    }

    #[test]
    fn test_parse_vector_text() {
        assert_eq!(parse_vector_text("[0.1, 0.2, 0.3]"), vec![0.1, 0.2, 0.3]);
        assert_eq!(
            parse_vector_text(" 0.5, -1.2, 3.15 "),
            vec![0.5, -1.2, 3.15]
        );
        assert_eq!(parse_vector_text("[]"), Vec::<f32>::new());
        assert_eq!(parse_vector_text(""), Vec::<f32>::new());
        assert_eq!(parse_vector_text("[invalid, 1.0]"), vec![1.0]);
    }

    #[test]
    fn test_parse_vector_binary_utf8_string() {
        let text_bytes = b"[0.15, -0.25, 0.35]";
        assert_eq!(parse_vector_binary(text_bytes), vec![0.15, -0.25, 0.35]);
    }

    #[test]
    fn test_parse_vector_binary_pgvector_wire_format() {
        let dim: u16 = 3;
        let mut wire_bytes = Vec::new();
        wire_bytes.extend_from_slice(&dim.to_be_bytes());
        wire_bytes.extend_from_slice(&0u16.to_be_bytes()); // unused/reserved 2 bytes
        for val in &[0.1f32, 0.2f32, 0.3f32] {
            wire_bytes.extend_from_slice(&val.to_be_bytes());
        }

        assert_eq!(parse_vector_binary(&wire_bytes), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn test_parse_vector_binary_raw_floats() {
        let floats = vec![1.0f32, 2.0f32, 3.0f32];
        let mut raw_bytes = Vec::new();
        for f in &floats {
            raw_bytes.extend_from_slice(&f.to_ne_bytes());
        }

        assert_eq!(parse_vector_binary(&raw_bytes), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_parse_vector_binary_empty_and_invalid() {
        assert_eq!(parse_vector_binary(&[]), Vec::<f32>::new());
        assert_eq!(parse_vector_binary(&[1, 2, 3]), Vec::<f32>::new());
    }
}
