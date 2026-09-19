//! SQLite store for document collections, entries, and chunks.
//!
//! Uses the same Xavier SQLite connection pattern (r2d2 + rusqlite) and
//! integrates with the existing `sqlite-vec` extension for chunk embeddings.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use serde_json::Value;
use tracing::{debug, info};

use crate::collections::schema::{CollectionAccess, DocChunk, DocCollection, DocEntry, DocStatus};

// ── Schema DDL ─────────────────────────────────────────────────────────────────

const SCHEMA_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS doc_collections (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    description TEXT,
    tags        TEXT NOT NULL DEFAULT '[]',   -- JSON array
    owner_id    TEXT,
    access      TEXT NOT NULL DEFAULT '"private"', -- JSON
    language    TEXT NOT NULL DEFAULT 'es',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_doc_collections_owner
    ON doc_collections (owner_id);

CREATE TABLE IF NOT EXISTS doc_entries (
    id            TEXT PRIMARY KEY NOT NULL,
    collection_id TEXT NOT NULL REFERENCES doc_collections(id) ON DELETE CASCADE,
    title         TEXT NOT NULL,
    file_name     TEXT,
    source_url    TEXT,
    mime_type     TEXT NOT NULL DEFAULT 'application/octet-stream',
    raw_size      INTEGER NOT NULL DEFAULT 0,
    chunk_count   INTEGER NOT NULL DEFAULT 0,
    version       INTEGER NOT NULL DEFAULT 1,
    status        TEXT NOT NULL DEFAULT '"pending"', -- JSON enum
    error_message TEXT,
    sha256        TEXT,
    metadata      TEXT NOT NULL DEFAULT '{}',
    ingested_at   TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_doc_entries_collection
    ON doc_entries (collection_id, status);

CREATE INDEX IF NOT EXISTS idx_doc_entries_sha256
    ON doc_entries (sha256);

CREATE TABLE IF NOT EXISTS doc_chunks (
    id            TEXT PRIMARY KEY NOT NULL,
    doc_id        TEXT NOT NULL REFERENCES doc_entries(id) ON DELETE CASCADE,
    collection_id TEXT NOT NULL,
    chunk_index   INTEGER NOT NULL,
    content       TEXT NOT NULL,
    page          INTEGER,
    breadcrumb    TEXT,
    token_count   INTEGER NOT NULL DEFAULT 0,
    metadata      TEXT NOT NULL DEFAULT '{}',
    UNIQUE (doc_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_doc_chunks_collection
    ON doc_chunks (collection_id);

CREATE INDEX IF NOT EXISTS idx_doc_chunks_doc
    ON doc_chunks (doc_id, chunk_index);

-- Full-text search on chunk content
CREATE VIRTUAL TABLE IF NOT EXISTS doc_chunks_fts USING fts5(
    content,
    chunk_id UNINDEXED,
    doc_id   UNINDEXED,
    collection_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 1'
);
"#;

// ── Store ──────────────────────────────────────────────────────────────────────

/// Thread-safe SQLite store for document collections.
#[derive(Clone)]
pub struct CollectionStore {
    pool: Arc<Pool<SqliteConnectionManager>>,
}

impl CollectionStore {
    /// Open (or create) the collection store at the given path.
    pub fn open(db_path: &PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir {:?}", parent))?;
        }

        let manager = SqliteConnectionManager::file(db_path).with_init(|conn| {
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;\
                     PRAGMA synchronous=NORMAL;\
                     PRAGMA foreign_keys=ON;\
                     PRAGMA temp_store=MEMORY;",
            )
        });

        let pool = Pool::builder()
            .max_size(8)
            .build(manager)
            .with_context(|| format!("building pool for {:?}", db_path))?;

        let store = Self {
            pool: Arc::new(pool),
        };
        store.run_migrations()?;

        info!("CollectionStore ready at {:?}", db_path);
        Ok(store)
    }

    /// Open the store using Xavier's default data directory.
    pub fn from_env() -> Result<Self> {
        let data_dir = crate::settings::XavierSettings::resolve_data_dir();
        let db_path = data_dir.join("docbot_collections.db");
        Self::open(&db_path)
    }

    fn run_migrations(&self) -> Result<()> {
        let conn = self.pool.get()?;
        conn.execute_batch(SCHEMA_DDL)
            .context("running collection store DDL migrations")?;
        Ok(())
    }

    // ── Collections ───────────────────────────────────────────────────────────

    /// Insert a new collection.
    pub fn create_collection(&self, col: &DocCollection) -> Result<()> {
        let conn = self.pool.get()?;
        let tags_json = serde_json::to_string(&col.tags)?;
        let access_json = serde_json::to_string(&col.access)?;

        conn.execute(
            "INSERT INTO doc_collections
             (id, name, description, tags, owner_id, access, language, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                col.id,
                col.name,
                col.description,
                tags_json,
                col.owner_id,
                access_json,
                col.language,
                col.created_at.to_rfc3339(),
                col.updated_at.to_rfc3339(),
            ],
        )
        .context("inserting doc_collection")?;

        debug!(id = %col.id, name = %col.name, "collection created");
        Ok(())
    }

    /// Fetch a collection by ID.
    pub fn get_collection(&self, id: &str) -> Result<Option<DocCollection>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, description, tags, owner_id, access, language, created_at, updated_at
             FROM doc_collections WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_collection(row)?))
        } else {
            Ok(None)
        }
    }

    /// List all collections (optionally filtered by owner).
    pub fn list_collections(&self, owner_id: Option<&str>) -> Result<Vec<DocCollection>> {
        let conn = self.pool.get()?;
        let (sql, bind_owner) = if owner_id.is_some() {
            (
                "SELECT id, name, description, tags, owner_id, access, language, created_at, updated_at
                 FROM doc_collections WHERE owner_id = ?1 ORDER BY created_at DESC",
                true,
            )
        } else {
            (
                "SELECT id, name, description, tags, owner_id, access, language, created_at, updated_at
                 FROM doc_collections ORDER BY created_at DESC",
                false,
            )
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = if bind_owner {
            stmt.query(params![owner_id.unwrap()])?
        } else {
            stmt.query([])?
        };

        rows_to_collections(rows)
    }

    /// Delete a collection and all its documents/chunks (CASCADE).
    pub fn delete_collection(&self, id: &str) -> Result<usize> {
        let conn = self.pool.get()?;
        let n = conn.execute("DELETE FROM doc_collections WHERE id = ?1", params![id])?;
        Ok(n)
    }

    // ── Documents ─────────────────────────────────────────────────────────────

    /// Insert a new document entry.
    pub fn create_doc(&self, doc: &DocEntry) -> Result<()> {
        let conn = self.pool.get()?;
        let status_json = serde_json::to_string(&doc.status)?;
        let meta_json = serde_json::to_string(&doc.metadata)?;

        conn.execute(
            "INSERT INTO doc_entries
             (id, collection_id, title, file_name, source_url, mime_type,
              raw_size, chunk_count, version, status, error_message, sha256,
              metadata, ingested_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                doc.id,
                doc.collection_id,
                doc.title,
                doc.file_name,
                doc.source_url,
                doc.mime_type,
                doc.raw_size as i64,
                doc.chunk_count as i64,
                doc.version as i64,
                status_json,
                doc.error_message,
                doc.sha256,
                meta_json,
                doc.ingested_at.to_rfc3339(),
                doc.updated_at.to_rfc3339(),
            ],
        )
        .context("inserting doc_entry")?;

        Ok(())
    }

    /// Update document status (and optionally chunk_count + error_message).
    pub fn update_doc_status(
        &self,
        doc_id: &str,
        status: &DocStatus,
        chunk_count: Option<u32>,
        error_msg: Option<&str>,
    ) -> Result<()> {
        let conn = self.pool.get()?;
        let status_json = serde_json::to_string(status)?;
        let now = Utc::now().to_rfc3339();

        if let Some(count) = chunk_count {
            conn.execute(
                "UPDATE doc_entries SET status=?1, chunk_count=?2, error_message=?3, updated_at=?4
                 WHERE id=?5",
                params![status_json, count as i64, error_msg, now, doc_id],
            )?;
        } else {
            conn.execute(
                "UPDATE doc_entries SET status=?1, error_message=?2, updated_at=?3 WHERE id=?4",
                params![status_json, error_msg, now, doc_id],
            )?;
        }

        Ok(())
    }

    /// List documents in a collection.
    pub fn list_docs(&self, collection_id: &str) -> Result<Vec<DocEntry>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, collection_id, title, file_name, source_url, mime_type,
                    raw_size, chunk_count, version, status, error_message, sha256,
                    metadata, ingested_at, updated_at
             FROM doc_entries WHERE collection_id = ?1 ORDER BY ingested_at DESC",
        )?;

        let rows = stmt.query(params![collection_id])?;
        rows_to_docs(rows)
    }

    /// Fetch a single doc by ID.
    pub fn get_doc(&self, doc_id: &str) -> Result<Option<DocEntry>> {
        let conn = self.pool.get()?;
        let mut stmt = conn.prepare(
            "SELECT id, collection_id, title, file_name, source_url, mime_type,
                    raw_size, chunk_count, version, status, error_message, sha256,
                    metadata, ingested_at, updated_at
             FROM doc_entries WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![doc_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_doc(row)?))
        } else {
            Ok(None)
        }
    }

    // ── Chunks ────────────────────────────────────────────────────────────────

    /// Insert a batch of chunks for a document.
    pub fn insert_chunks(&self, chunks: &[DocChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let conn = self.pool.get()?;

        let tx = conn;
        tx.execute_batch("BEGIN IMMEDIATE")?;

        for chunk in chunks {
            let meta_json = serde_json::to_string(&chunk.metadata)?;
            tx.execute(
                "INSERT OR IGNORE INTO doc_chunks
                 (id, doc_id, collection_id, chunk_index, content, page,
                  breadcrumb, token_count, metadata)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    chunk.id,
                    chunk.doc_id,
                    chunk.collection_id,
                    chunk.chunk_index as i64,
                    chunk.content,
                    chunk.page.map(|p| p as i64),
                    chunk.breadcrumb,
                    chunk.token_count as i64,
                    meta_json,
                ],
            )?;

            // Mirror into FTS table
            tx.execute(
                "INSERT OR IGNORE INTO doc_chunks_fts (content, chunk_id, doc_id, collection_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![chunk.content, chunk.id, chunk.doc_id, chunk.collection_id],
            )?;
        }

        tx.execute_batch("COMMIT")?;
        debug!("inserted {} chunks", chunks.len());
        Ok(())
    }

    /// BM25 full-text search over chunks in one or all collections.
    pub fn fts_search(
        &self,
        query: &str,
        collection_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChunkSearchResult>> {
        let safe_query = match crate::memory::sqlite_vec_store::fts::build_fts_query(query) {
            Some(q) => q,
            None => return Ok(Vec::new()),
        };
        let query = &safe_query;
        let conn = self.pool.get()?;

        let (sql, use_collection) = if collection_id.is_some() {
            (
                "SELECT f.chunk_id, f.doc_id, f.collection_id, c.content,
                        c.page, c.breadcrumb, bm25(doc_chunks_fts) AS score
                 FROM doc_chunks_fts f
                 JOIN doc_chunks c ON c.id = f.chunk_id
                 WHERE doc_chunks_fts MATCH ?1 AND f.collection_id = ?2
                 ORDER BY score LIMIT ?3",
                true,
            )
        } else {
            (
                "SELECT f.chunk_id, f.doc_id, f.collection_id, c.content,
                        c.page, c.breadcrumb, bm25(doc_chunks_fts) AS score
                 FROM doc_chunks_fts f
                 JOIN doc_chunks c ON c.id = f.chunk_id
                 WHERE doc_chunks_fts MATCH ?1
                 ORDER BY score LIMIT ?2",
                false,
            )
        };

        let mut stmt = conn.prepare(sql)?;

        let rows = if use_collection {
            stmt.query(params![query, collection_id.unwrap(), limit as i64])?
        } else {
            stmt.query(params![query, limit as i64])?
        };

        let mut results = Vec::new();
        let mut rows = rows;
        while let Some(row) = rows.next()? {
            results.push(ChunkSearchResult {
                chunk_id: row.get(0)?,
                doc_id: row.get(1)?,
                collection_id: row.get(2)?,
                content: row.get(3)?,
                page: row.get::<_, Option<i64>>(4)?.map(|p| p as u32),
                breadcrumb: row.get(5)?,
                score: row.get::<_, f64>(6)? as f32,
            });
        }

        Ok(results)
    }

    /// Fetch chunks by their IDs (used after vector search).
    pub fn get_chunks_by_ids(&self, ids: &[&str]) -> Result<Vec<DocChunk>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.pool.get()?;
        let placeholders = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(",");

        let sql = format!(
            "SELECT id, doc_id, collection_id, chunk_index, content, page,
                    breadcrumb, token_count, metadata
             FROM doc_chunks WHERE id IN ({})",
            placeholders
        );

        let mut stmt = conn.prepare(&sql)?;
        let params_iter: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let rows = stmt.query(params_iter.as_slice())?;
        rows_to_chunks(rows)
    }
}

// ── Search Result ──────────────────────────────────────────────────────────────

/// A chunk returned by BM25 or vector search.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChunkSearchResult {
    pub chunk_id: String,
    pub doc_id: String,
    pub collection_id: String,
    pub content: String,
    pub page: Option<u32>,
    pub breadcrumb: Option<String>,
    pub score: f32,
}

// ── Row helpers ────────────────────────────────────────────────────────────────

fn row_to_collection(row: &rusqlite::Row<'_>) -> Result<DocCollection> {
    let tags_json: String = row.get(3)?;
    let access_json: String = row.get(5)?;
    let created_str: String = row.get(7)?;
    let updated_str: String = row.get(8)?;

    Ok(DocCollection {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        owner_id: row.get(4)?,
        access: serde_json::from_str(&access_json).unwrap_or(CollectionAccess::Private),
        language: row.get(6)?,
        created_at: DateTime::parse_from_rfc3339(&created_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

fn rows_to_collections(mut rows: rusqlite::Rows<'_>) -> Result<Vec<DocCollection>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_collection(row)?);
    }
    Ok(out)
}

fn row_to_doc(row: &rusqlite::Row<'_>) -> Result<DocEntry> {
    let status_json: String = row.get(9)?;
    let meta_json: String = row.get(12)?;
    let ingested_str: String = row.get(13)?;
    let updated_str: String = row.get(14)?;

    Ok(DocEntry {
        id: row.get(0)?,
        collection_id: row.get(1)?,
        title: row.get(2)?,
        file_name: row.get(3)?,
        source_url: row.get(4)?,
        mime_type: row.get(5)?,
        raw_size: row.get::<_, i64>(6)? as u64,
        chunk_count: row.get::<_, i64>(7)? as u32,
        version: row.get::<_, i64>(8)? as u32,
        status: serde_json::from_str(&status_json).unwrap_or(DocStatus::Pending),
        error_message: row.get(10)?,
        sha256: row.get(11)?,
        metadata: serde_json::from_str(&meta_json).unwrap_or_default(),
        ingested_at: DateTime::parse_from_rfc3339(&ingested_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        updated_at: DateTime::parse_from_rfc3339(&updated_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

fn rows_to_docs(mut rows: rusqlite::Rows<'_>) -> Result<Vec<DocEntry>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_doc(row)?);
    }
    Ok(out)
}

fn row_to_chunk(row: &rusqlite::Row<'_>) -> Result<DocChunk> {
    let meta_json: String = row.get(8)?;
    Ok(DocChunk {
        id: row.get(0)?,
        doc_id: row.get(1)?,
        collection_id: row.get(2)?,
        chunk_index: row.get::<_, i64>(3)? as u32,
        content: row.get(4)?,
        page: row.get::<_, Option<i64>>(5)?.map(|p| p as u32),
        breadcrumb: row.get(6)?,
        token_count: row.get::<_, i64>(7)? as u32,
        metadata: serde_json::from_str(&meta_json).unwrap_or_default(),
    })
}

fn rows_to_chunks(mut rows: rusqlite::Rows<'_>) -> Result<Vec<DocChunk>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_chunk(row)?);
    }
    Ok(out)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_temp_store() -> CollectionStore {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_collections.db");
        CollectionStore::open(&path).expect("open store")
    }

    #[test]
    fn test_create_and_get_collection() {
        let store = open_temp_store();
        let col = DocCollection::new("Test Collection");
        store.create_collection(&col).unwrap();

        let fetched = store.get_collection(&col.id).unwrap().unwrap();
        assert_eq!(fetched.name, "Test Collection");
        assert_eq!(fetched.language, "es");
    }

    #[test]
    fn test_list_collections_empty() {
        let store = open_temp_store();
        let cols = store.list_collections(None).unwrap();
        assert!(cols.is_empty());
    }

    #[test]
    fn test_create_and_list_docs() {
        let store = open_temp_store();
        let col = DocCollection::new("Docs");
        store.create_collection(&col).unwrap();

        let doc = DocEntry::new(&col.id, "My PDF", "application/pdf");
        store.create_doc(&doc).unwrap();

        let docs = store.list_docs(&col.id).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "My PDF");
    }

    #[test]
    fn test_update_doc_status() {
        let store = open_temp_store();
        let col = DocCollection::new("Col");
        store.create_collection(&col).unwrap();

        let doc = DocEntry::new(&col.id, "Doc", "text/plain");
        store.create_doc(&doc).unwrap();

        store
            .update_doc_status(&doc.id, &DocStatus::Ready, Some(42), None)
            .unwrap();

        let fetched = store.get_doc(&doc.id).unwrap().unwrap();
        assert_eq!(fetched.status, DocStatus::Ready);
        assert_eq!(fetched.chunk_count, 42);
    }

    #[test]
    fn test_insert_and_fts_search() {
        let store = open_temp_store();
        let col = DocCollection::new("Legal");
        store.create_collection(&col).unwrap();

        let doc = DocEntry::new(&col.id, "Contract", "application/pdf");
        store.create_doc(&doc).unwrap();

        let chunk = DocChunk::new(
            &doc.id,
            &col.id,
            0,
            "Esta es una cláusula contractual importante.",
        )
        .with_page(1)
        .with_breadcrumb("Artículo 5");
        store.insert_chunks(&[chunk]).unwrap();

        let results = store.fts_search("cláusula contractual", None, 10).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].content.contains("cláusula"));
    }

    #[test]
    fn test_delete_collection_cascades() {
        let store = open_temp_store();
        let col = DocCollection::new("To Delete");
        store.create_collection(&col).unwrap();

        let doc = DocEntry::new(&col.id, "Doc", "text/plain");
        store.create_doc(&doc).unwrap();

        let n = store.delete_collection(&col.id).unwrap();
        assert_eq!(n, 1);

        // Doc should be gone (CASCADE)
        let docs = store.list_docs(&col.id).unwrap();
        assert!(docs.is_empty());
    }
}
