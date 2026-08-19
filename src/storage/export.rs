//! Streamable vector exporter for SQLite vector stores.
//!
//! Provides [`VectorExporter`] to stream vector embeddings from SQLite databases
//! into JSONL and Parquet formats while maintaining strict memory bounds
//! (max 1000 items in memory at any point).

use anyhow::{Context, Result};
use parquet::data_type::ByteArrayType;
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncWriteExt, BufWriter};

/// Supported vector export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// JSON Lines format (one JSON record per line).
    Jsonl,
    /// Apache Parquet binary format.
    Parquet,
}

/// A single exported vector record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorExportRecord {
    pub id: String,
    pub workspace_id: String,
    pub path: String,
    pub content: String,
    pub metadata: serde_json::Value,
    pub embedding: Vec<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Streamable exporter for vector stores.
#[derive(Debug, Clone)]
pub struct VectorExporter {
    db_path: PathBuf,
    batch_size: usize,
}

impl VectorExporter {
    /// Create a new exporter for the given SQLite database path.
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            batch_size: 1000,
        }
    }

    /// Set the batch size for streaming (clamped to a maximum of 1000 items).
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.clamp(1, 1000);
        self
    }

    /// Export vector embeddings to the specified path in the given format.
    pub async fn export(&self, path: &Path, format: ExportFormat) -> Result<usize> {
        match format {
            ExportFormat::Jsonl => self.export_jsonl(path).await,
            ExportFormat::Parquet => self.export_parquet(path).await,
        }
    }

    /// Export vector embeddings into JSON Lines (`.jsonl`) format.
    pub async fn export_jsonl(&self, path: &Path) -> Result<usize> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = TokioFile::create(path)
            .await
            .with_context(|| format!("Failed to create export JSONL file at {:?}", path))?;
        let mut writer = BufWriter::new(file);

        let mut offset = 0;
        let mut total_exported = 0;
        let batch_size = self.batch_size.min(1000);

        loop {
            let db_path = self.db_path.clone();
            let batch = tokio::task::spawn_blocking(move || -> Result<Vec<VectorExportRecord>> {
                Self::fetch_batch_sync(&db_path, offset, batch_size)
            })
            .await
            .context("Task join error during batch fetch")??;

            if batch.is_empty() {
                break;
            }

            for record in &batch {
                let json_line = serde_json::to_string(record)
                    .context("Failed to serialize VectorExportRecord")?;
                writer.write_all(json_line.as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }

            let batch_len = batch.len();
            total_exported += batch_len;
            offset += batch_len;

            if batch_len < batch_size {
                break;
            }
        }

        writer.flush().await.context("Failed to flush JSONL output buffer")?;
        Ok(total_exported)
    }

    /// Export vector embeddings into Parquet (`.parquet`) binary format.
    pub async fn export_parquet(&self, path: &Path) -> Result<usize> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let db_path = self.db_path.clone();
        let target_path = path.to_path_buf();
        let batch_size = self.batch_size.min(1000);

        tokio::task::spawn_blocking(move || -> Result<usize> {
            Self::export_parquet_sync(&db_path, &target_path, batch_size)
        })
        .await
        .context("Task join error during Parquet export")?
    }

    /// Synchronously export records to a standard Parquet file using `parquet` crate.
    fn export_parquet_sync(db_path: &Path, path: &Path, batch_size: usize) -> Result<usize> {
        let file = File::create(path)
            .with_context(|| format!("Failed to create Parquet file at {:?}", path))?;

        let schema_str = "
          message VectorRecord {
            REQUIRED BYTE_ARRAY id (UTF8);
            REQUIRED BYTE_ARRAY workspace_id (UTF8);
            REQUIRED BYTE_ARRAY path (UTF8);
            REQUIRED BYTE_ARRAY content (UTF8);
            REQUIRED BYTE_ARRAY metadata (UTF8);
            OPTIONAL BYTE_ARRAY created_at (UTF8);
            OPTIONAL BYTE_ARRAY updated_at (UTF8);
            REQUIRED BYTE_ARRAY embedding (UTF8);
          }
        ";

        let schema = Arc::new(
            parse_message_type(schema_str)
                .context("Failed to parse Parquet schema message type")?,
        );

        let props = Arc::new(WriterProperties::builder().build());
        let mut writer = SerializedFileWriter::new(file, schema, props)
            .context("Failed to initialize Parquet SerializedFileWriter")?;

        let mut offset = 0;
        let mut total_exported = 0;

        loop {
            let batch = Self::fetch_batch_sync(db_path, offset, batch_size)?;
            if batch.is_empty() {
                break;
            }

            let mut row_group_writer = writer
                .next_row_group()
                .context("Failed to create next row group in Parquet file")?;

            let mut ids: Vec<parquet::data_type::ByteArray> = Vec::with_capacity(batch.len());
            let mut workspace_ids: Vec<parquet::data_type::ByteArray> = Vec::with_capacity(batch.len());
            let mut paths: Vec<parquet::data_type::ByteArray> = Vec::with_capacity(batch.len());
            let mut contents: Vec<parquet::data_type::ByteArray> = Vec::with_capacity(batch.len());
            let mut metadatas: Vec<parquet::data_type::ByteArray> = Vec::with_capacity(batch.len());
            let mut created_ats: Vec<Option<parquet::data_type::ByteArray>> = Vec::with_capacity(batch.len());
            let mut updated_ats: Vec<Option<parquet::data_type::ByteArray>> = Vec::with_capacity(batch.len());
            let mut embeddings: Vec<parquet::data_type::ByteArray> = Vec::with_capacity(batch.len());

            for record in &batch {
                ids.push(record.id.as_str().into());
                workspace_ids.push(record.workspace_id.as_str().into());
                paths.push(record.path.as_str().into());
                contents.push(record.content.as_str().into());
                metadatas.push(record.metadata.to_string().as_str().into());
                created_ats.push(record.created_at.as_deref().map(Into::into));
                updated_ats.push(record.updated_at.as_deref().map(Into::into));
                let emb_str = serde_json::to_string(&record.embedding).unwrap_or_default();
                embeddings.push(emb_str.as_str().into());
            }

            // Write column by column
            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                w.write_batch(&ids, None, None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                w.write_batch(&workspace_ids, None, None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                w.write_batch(&paths, None, None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                w.write_batch(&contents, None, None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                w.write_batch(&metadatas, None, None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                let mut values = Vec::new();
                let mut def_levels = Vec::new();
                for opt in &created_ats {
                    if let Some(val) = opt {
                        values.push(val.clone());
                        def_levels.push(1i16);
                    } else {
                        def_levels.push(0i16);
                    }
                }
                w.write_batch(&values, Some(&def_levels), None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                let mut values = Vec::new();
                let mut def_levels = Vec::new();
                for opt in &updated_ats {
                    if let Some(val) = opt {
                        values.push(val.clone());
                        def_levels.push(1i16);
                    } else {
                        def_levels.push(0i16);
                    }
                }
                w.write_batch(&values, Some(&def_levels), None)?;
                col_writer.close()?;
            }

            if let Some(mut col_writer) = row_group_writer.next_column()? {
                let w = col_writer.typed::<ByteArrayType>();
                w.write_batch(&embeddings, None, None)?;
                col_writer.close()?;
            }

            row_group_writer
                .close()
                .context("Failed to close Parquet row group writer")?;

            let batch_len = batch.len();
            total_exported += batch_len;
            offset += batch_len;

            if batch_len < batch_size {
                break;
            }
        }

        writer
            .close()
            .context("Failed to close Parquet file writer")?;

        Ok(total_exported)
    }

    /// Fetch a batch of records synchronously from SQLite.
    fn fetch_batch_sync(
        db_path: &Path,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<VectorExportRecord>> {
        if !db_path.exists() {
            return Ok(Vec::new());
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open SQLite database at {:?}", db_path))?;

        let has_memory_records: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_records'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !has_memory_records {
            return Ok(Vec::new());
        }

        let has_vec_embeddings: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_embeddings'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        let has_vec_embeddings_768: bool = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_embeddings_768'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        let mut records = Vec::with_capacity(limit);

        if has_vec_embeddings && has_vec_embeddings_768 {
            let sql = "SELECT r.id, r.workspace_id, r.path, r.content, r.metadata, \
                       COALESCE(r.embedding, e1.embedding, e2.embedding) as embedding, \
                       r.created_at, r.updated_at \
                       FROM memory_records r \
                       LEFT JOIN memory_embeddings e1 ON r.id = e1.id \
                       LEFT JOIN memory_embeddings_768 e2 ON r.id = e2.id \
                       ORDER BY r.id \
                       LIMIT ?1 OFFSET ?2";

            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![limit as i64, offset as i64], Self::map_row)?;
            for row in rows {
                records.push(row?);
            }
        } else if has_vec_embeddings {
            let sql = "SELECT r.id, r.workspace_id, r.path, r.content, r.metadata, \
                       COALESCE(r.embedding, e1.embedding) as embedding, \
                       r.created_at, r.updated_at \
                       FROM memory_records r \
                       LEFT JOIN memory_embeddings e1 ON r.id = e1.id \
                       ORDER BY r.id \
                       LIMIT ?1 OFFSET ?2";

            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![limit as i64, offset as i64], Self::map_row)?;
            for row in rows {
                records.push(row?);
            }
        } else {
            let sql = "SELECT id, workspace_id, path, content, metadata, embedding, created_at, updated_at \
                       FROM memory_records \
                       ORDER BY id \
                       LIMIT ?1 OFFSET ?2";

            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![limit as i64, offset as i64], Self::map_row)?;
            for row in rows {
                records.push(row?);
            }
        }

        Ok(records)
    }

    /// Helper to map a SQLite row into a `VectorExportRecord`.
    fn map_row(row: &rusqlite::Row) -> rusqlite::Result<VectorExportRecord> {
        let id: String = row.get(0)?;
        let workspace_id: String = row.get(1)?;
        let path: String = row.get(2)?;
        let content: String = row.get(3)?;

        let raw_metadata: Option<String> = row.get(4)?;
        let metadata: serde_json::Value = match raw_metadata {
            Some(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({})),
            None => serde_json::json!({}),
        };

        let raw_embedding: Option<Vec<u8>> = row.get(5)?;
        let embedding = match raw_embedding {
            Some(bytes) if !bytes.is_empty() => {
                crate::memory::sqlite_vec_store::vector::deserialize_embedding(&bytes)
            }
            _ => Vec::new(),
        };

        let created_at: Option<String> = row.get(6)?;
        let updated_at: Option<String> = row.get(7)?;

        Ok(VectorExportRecord {
            id,
            workspace_id,
            path,
            content,
            metadata,
            embedding,
            created_at,
            updated_at,
        })
    }
}
