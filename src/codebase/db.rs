//! Per-project codebase database manager.
//!
//! Creates and manages tables in `.xavier/codebase.db`.

use std::path::Path;

use anyhow::{Context, Result};
use libsql::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// A single row from an FTS5 code search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSearchResult {
    pub path: String,
    pub content: String,
    pub code_tokens: String,
    pub rank: f64,
}

/// A single row from a semantic (vector) search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub id: String,
    pub path: String,
    pub content: String,
    pub distance: f64,
}

/// Result from hybrid (FTS + vector) search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridResult {
    pub path: String,
    pub content: String,
    pub score: f64,
}

/// Input for batch inserting code symbols.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInput {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: Option<String>,
    pub visibility: Option<String>,
    pub doc_comment: Option<String>,
    pub language: String,
    pub module_path: Option<String>,
    pub complexity: Option<f64>,
}

/// Input for batch inserting code chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkInput {
    pub id: String,
    pub path: String,
    pub content: String,
    pub language: Option<String>,
    pub symbol_id: Option<String>,
    pub tokens: Option<i64>,
}

/// Input for batch inserting embeddings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingInput {
    pub id: String,
    pub embedding: Vec<f32>,
}

// ---------------------------------------------------------------------------
// CodebaseDb
// ---------------------------------------------------------------------------

/// Manages the per-project codebase libSQL database.
pub struct CodebaseDb {
    conn: Connection,
    schema_initialized: OnceCell<()>,
}

impl CodebaseDb {
    /// Open (or create) the codebase database at `path`.
    pub async fn open(path: &Path) -> Result<Self> {
        let path_str = path.to_string_lossy().to_string();
        let db = libsql::Builder::new_local(&path_str)
            .build()
            .await
            .with_context(|| format!("failed to open codebase database at {}", path.display()))?;

        let conn = db.connect().context("failed to connect to libSQL database")?;
        Self::enable_pragmas(&conn).await?;
        Ok(Self { conn, schema_initialized: OnceCell::new() })
    }

    /// Open an in-memory database (for testing).
    pub async fn open_in_memory() -> Result<Self> {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .context("failed to build in-memory libSQL database")?;
        let conn = db.connect().context("failed to connect to in-memory libSQL database")?;
        Self::enable_pragmas(&conn).await?;
        Ok(Self { conn, schema_initialized: OnceCell::new() })
    }

    /// Ensure the schema is created.
    async fn ensure_schema(&self) -> Result<()> {
        self.schema_initialized.get_or_try_init(|| async {
            self.create_schema().await
        }).await?;
        Ok(())
    }

    /// Create (or migrate) the schema.
    ///
    /// All tables are created regardless of any external configuration.
    pub async fn create_schema(&self) -> Result<()> {
        let stmts = vec![
            create_repo_meta_table(),
            create_git_commits_table(),
            create_git_files_table(),
            create_git_blame_table(),
            create_symbols_table(),
            create_symbol_relations_table(),
            create_imports_table(),
            create_patterns_table(),
            create_code_chunks_table(),
            create_code_embeddings_table(),
            create_code_fts_table(),
        ];

        for stmt in &stmts {
            self.conn
                .execute_batch(stmt)
                .await
                .with_context(|| format!("failed to execute schema SQL:\n{}", stmt))?;
        }
        Ok(())
    }

    /// Return a reference to the underlying connection.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    // ------------------------------------------------------------------
    // Insert helpers
    // ------------------------------------------------------------------

    /// Insert a single git commit record.
    pub async fn insert_commit(
        &self, hash: &str, author: &str, date: &str, message: &str,
        branch: Option<&str>, parents: &[&str],
    ) -> Result<()> {
        self.ensure_schema().await?;
        let parents_json = serde_json::to_string(parents)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO git_commits (hash, author, date, message, branch, parents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![hash, author, date, message, branch, parents_json],
        ).await.context("failed to insert git commit")?;
        Ok(())
    }

    /// Insert or update a file record.
    pub async fn insert_file(
        &self, path: &str, added_at: Option<&str>, last_modified: Option<&str>,
        loc: i64, language: Option<&str>, module_path: Option<&str>,
    ) -> Result<()> {
        self.ensure_schema().await?;
        self.conn.execute(
            "INSERT OR REPLACE INTO git_files (path, added_at, last_modified, loc, language, module_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![path, added_at, last_modified, loc, language, module_path],
        ).await.context("failed to insert file record")?;
        Ok(())
    }

    /// Insert a blame line range.
    pub async fn insert_blame(
        &self, file_path: &str, line_start: i64, line_end: i64,
        commit_hash: &str, author: &str, date: &str,
    ) -> Result<()> {
        self.ensure_schema().await?;
        self.conn.execute(
            "INSERT OR REPLACE INTO git_blame (file_path, line_start, line_end, commit_hash, author, date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![file_path, line_start, line_end, commit_hash, author, date],
        ).await.context("failed to insert git blame")?;
        Ok(())
    }

    /// Insert a code symbol.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_symbol(
        &self, id: &str, name: &str, kind: &str, file_path: &str,
        line_start: i64, line_end: i64, signature: Option<&str>,
        visibility: Option<&str>, doc_comment: Option<&str>,
        language: &str, module_path: Option<&str>, complexity: Option<f64>,
    ) -> Result<()> {
        self.ensure_schema().await?;
        self.conn.execute(
            "INSERT OR REPLACE INTO symbols
             (id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity],
        ).await.context("failed to insert symbol")?;
        Ok(())
    }

    /// Insert a symbol-to-symbol relation.
    pub async fn insert_relation(
        &self, source_id: &str, target_id: &str, relation: &str, file_path: Option<&str>,
    ) -> Result<()> {
        self.ensure_schema().await?;
        self.conn.execute(
            "INSERT OR REPLACE INTO symbol_relations (source_id, target_id, relation, file_path)
             VALUES (?1, ?2, ?3, ?4)",
            params![source_id, target_id, relation, file_path],
        ).await.context("failed to insert symbol relation")?;
        Ok(())
    }

    /// Insert a code chunk with optional embedding and FTS index content.
    pub async fn insert_chunk(
        &self, id: &str, path: &str, content: &str,
        language: Option<&str>, symbol_id: Option<&str>, tokens: Option<i64>,
    ) -> Result<()> {
        self.ensure_schema().await?;
        self.conn.execute(
            "INSERT OR REPLACE INTO code_chunks (id, path, content, language, symbol_id, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, path, content, language, symbol_id, tokens],
        ).await.context("failed to insert code chunk")?;
        Ok(())
    }

    /// Insert an embedding vector.
    pub async fn insert_embedding(&self, id: &str, embedding: &[f32]) -> Result<()> {
        self.ensure_schema().await?;
        let embedding_blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(embedding);
        self.conn.execute(
            "INSERT INTO code_embeddings (id, embedding) VALUES (?1, ?2)",
            params![id, embedding_blob],
        ).await.context("failed to insert embedding")?;
        Ok(())
    }

    /// Batch insert code symbols.
    pub async fn insert_symbols_batch(&self, symbols: &[SymbolInput]) -> Result<()> {
        self.ensure_schema().await?;
        let tx = self.conn.transaction().await?;
        for s in symbols {
            tx.execute(
                "INSERT OR REPLACE INTO symbols
                 (id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    s.id.clone(), s.name.clone(), s.kind.clone(), s.file_path.clone(),
                    s.line_start, s.line_end, s.signature.clone(), s.visibility.clone(),
                    s.doc_comment.clone(), s.language.clone(), s.module_path.clone(), s.complexity
                ],
            ).await?;
        }
        tx.commit().await.context("failed to commit symbols batch")?;
        Ok(())
    }

    /// Batch insert code chunks.
    pub async fn insert_chunks_batch(&self, chunks: &[ChunkInput]) -> Result<()> {
        self.ensure_schema().await?;
        let tx = self.conn.transaction().await?;
        for c in chunks {
            tx.execute(
                "INSERT OR REPLACE INTO code_chunks (id, path, content, language, symbol_id, tokens)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![c.id.clone(), c.path.clone(), c.content.clone(), c.language.clone(), c.symbol_id.clone(), c.tokens],
            ).await?;
        }
        tx.commit().await.context("failed to commit chunks batch")?;
        Ok(())
    }

    /// Batch insert embeddings.
    pub async fn insert_embeddings_batch(&self, embeddings: &[EmbeddingInput]) -> Result<()> {
        self.ensure_schema().await?;
        let tx = self.conn.transaction().await?;

        // Offload serialization to blocking thread for large batches
        let embeddings_with_blobs = if embeddings.len() > 100 {
            let embeddings_cloned = embeddings.to_vec();
            tokio::task::spawn_blocking(move || {
                embeddings_cloned.into_iter().map(|e| {
                    let blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(&e.embedding);
                    (e.id, blob)
                }).collect::<Vec<_>>()
            }).await.context("spawn_blocking for embedding serialization failed")?
        } else {
            embeddings.iter().map(|e| {
                let blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(&e.embedding);
                (e.id.clone(), blob)
            }).collect::<Vec<_>>()
        };

        for (id, blob) in embeddings_with_blobs {
            tx.execute(
                "INSERT INTO code_embeddings (id, embedding) VALUES (?1, ?2)",
                params![id, blob],
            ).await?;
        }
        tx.commit().await.context("failed to commit embeddings batch")?;
        Ok(())
    }

    /// Insert a pattern record.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_pattern(
        &self,
        id: &str,
        category: &str,
        pattern: &str,
        project: &str,
        discovered_by: &str,
        confidence: f64,
        source_file: Option<&str>,
        source_occurrences: i64,
        source_snippet: Option<&str>,
        verification: &str,
    ) -> Result<()> {
        self.ensure_schema().await?;
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR REPLACE INTO code_patterns (id, category, pattern, project, discovered_by, confidence, \
             source_file, source_occurrences, source_snippet, created_at, updated_at, usage_count, verification)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                id, category, pattern, project, discovered_by, confidence,
                source_file, source_occurrences, source_snippet, now.clone(), now, 0, verification
            ],
        ).await.context("failed to insert pattern")?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Search helpers
    // ------------------------------------------------------------------

    /// Full-text search over code via the FTS5 virtual table.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<CodeSearchResult>> {
        self.ensure_schema().await?;
        let sql = format!(
            "SELECT path, content, code_tokens, rank
             FROM code_fts
             WHERE code_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        );
        let mut rows = self.conn.query(&sql, params![query, limit as i64]).await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(CodeSearchResult {
                path: row.get(0)?, content: row.get(1)?,
                code_tokens: row.get(2)?, rank: row.get(3)?,
            });
        }
        Ok(results)
    }

    /// Semantic (vector) similarity search.
    pub async fn search_semantic(&self, embedding: &[f32], limit: usize) -> Result<Vec<SemanticSearchResult>> {
        self.ensure_schema().await?;
        let embedding_blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(embedding);
        let sql = format!(
            "SELECT ce.id, cc.path, cc.content, vector_distance_cos(ce.embedding, ?1) as distance
             FROM code_embeddings ce
             JOIN code_chunks cc ON cc.id = ce.id
             ORDER BY distance
             LIMIT ?2"
        );
        let mut rows = self.conn.query(&sql, params![embedding_blob, limit as i64]).await?;
        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            results.push(SemanticSearchResult {
                id: row.get(0)?, path: row.get(1)?,
                content: row.get(2)?, distance: row.get(3)?,
            });
        }
        Ok(results)
    }

    /// Hybrid search combining FTS5 and vector results (RRF-style).
    pub async fn hybrid_search(
        &self, text_query: &str, embedding: &[f32], limit: usize, rrf_k: usize,
    ) -> Result<Vec<HybridResult>> {
        self.ensure_schema().await?;
        use std::collections::HashMap;
        let mut scores: HashMap<String, HybridResult> = HashMap::new();

        let fts_results = self.search_code(text_query, limit * 2).await?;
        for (rank, result) in fts_results.iter().enumerate() {
            let rr = 1.0 / (rrf_k as f64 + rank as f64 + 1.0);
            scores.entry(result.path.clone())
                .and_modify(|e| e.score += rr)
                .or_insert(HybridResult { path: result.path.clone(), content: result.content.clone(), score: rr });
        }

        let vec_results = self.search_semantic(embedding, limit * 2).await?;
        for (rank, result) in vec_results.iter().enumerate() {
            let rr = 1.0 / (rrf_k as f64 + rank as f64 + 1.0);
            scores.entry(result.path.clone())
                .and_modify(|e| e.score += rr)
                .or_insert(HybridResult { path: result.path.clone(), content: result.content.clone(), score: rr });
        }

        let mut results: Vec<HybridResult> = scores.into_values().collect();
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        Ok(results)
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    async fn enable_pragmas(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        ).await.context("failed to set PRAGMAs")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Schema builders
// ---------------------------------------------------------------------------

fn create_repo_meta_table() -> String {
    "CREATE TABLE IF NOT EXISTS repo_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);".to_string()
}

fn create_git_commits_table() -> String {
    "CREATE TABLE IF NOT EXISTS git_commits (hash TEXT PRIMARY KEY, author TEXT NOT NULL, date TEXT NOT NULL, message TEXT NOT NULL, branch TEXT, parents TEXT);".to_string()
}

fn create_git_files_table() -> String {
    "CREATE TABLE IF NOT EXISTS git_files (path TEXT PRIMARY KEY, added_at TEXT, last_modified TEXT, loc INTEGER DEFAULT 0, language TEXT, module_path TEXT);".to_string()
}

fn create_git_blame_table() -> String {
    "CREATE TABLE IF NOT EXISTS git_blame (file_path TEXT NOT NULL, line_start INTEGER NOT NULL, line_end INTEGER NOT NULL, commit_hash TEXT NOT NULL, author TEXT NOT NULL, date TEXT NOT NULL, PRIMARY KEY (file_path, line_start, commit_hash));".to_string()
}

fn create_symbols_table() -> String {
    "CREATE TABLE IF NOT EXISTS symbols (id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, file_path TEXT NOT NULL, line_start INTEGER NOT NULL, line_end INTEGER NOT NULL, signature TEXT, visibility TEXT DEFAULT 'pub', doc_comment TEXT, language TEXT NOT NULL, module_path TEXT, complexity REAL);".to_string()
}

fn create_symbol_relations_table() -> String {
    "CREATE TABLE IF NOT EXISTS symbol_relations (source_id TEXT NOT NULL, target_id TEXT NOT NULL, relation TEXT NOT NULL, file_path TEXT, PRIMARY KEY (source_id, target_id, relation), FOREIGN KEY (source_id) REFERENCES symbols(id), FOREIGN KEY (target_id) REFERENCES symbols(id));".to_string()
}

fn create_imports_table() -> String {
    "CREATE TABLE IF NOT EXISTS imports (file_path TEXT NOT NULL, imported_symbol TEXT NOT NULL, source TEXT NOT NULL, alias TEXT);".to_string()
}

fn create_patterns_table() -> String {
    "CREATE TABLE IF NOT EXISTS code_patterns (
        id TEXT PRIMARY KEY,
        category TEXT NOT NULL,
        pattern TEXT NOT NULL,
        project TEXT NOT NULL,
        discovered_by TEXT NOT NULL DEFAULT 'auto',
        confidence REAL NOT NULL DEFAULT 0.5,
        source_file TEXT DEFAULT '',
        source_occurrences INTEGER DEFAULT 0,
        source_snippet TEXT DEFAULT '',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        usage_count INTEGER DEFAULT 0,
        verification TEXT DEFAULT 'pending'
    );".to_string()
}

fn create_code_chunks_table() -> String {
    "CREATE TABLE IF NOT EXISTS code_chunks (id TEXT PRIMARY KEY, path TEXT NOT NULL, content TEXT NOT NULL, embedding BLOB, language TEXT, symbol_id TEXT, tokens INTEGER, FOREIGN KEY (symbol_id) REFERENCES symbols(id));".to_string()
}

fn create_code_embeddings_table() -> String {
    "CREATE TABLE IF NOT EXISTS code_embeddings (id TEXT PRIMARY KEY, embedding F32_BLOB(384));".to_string()
}

fn create_code_fts_table() -> String {
    "CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(path, content, code_tokens, tokenize='porter unicode61');".to_string()
}

/// Populate code_fts from code_chunks (used after batch insert).
pub fn populate_fts_from_chunks_sql() -> String {
    "INSERT OR IGNORE INTO code_fts (path, content, code_tokens) SELECT path, content, '' FROM code_chunks;".to_string()
}

impl CodebaseDb {
    /// Populate the FTS index from code chunks.
    pub async fn populate_fts(&self) -> Result<()> {
        self.ensure_schema().await?;
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                conn.execute_batch(&populate_fts_from_chunks_sql())
                    .await
                    .context("failed to populate FTS index")?;
                Ok::<(), anyhow::Error>(())
            })
        }).await.context("spawn_blocking for FTS population failed")??;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_open_in_memory() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        let mut rows = db.conn
            .query("SELECT COUNT(*) FROM repo_meta", ())
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_create_schema_all_tables() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        let tables = get_table_names(&db.conn).await;
        assert!(tables.contains(&"repo_meta".to_string()));
        assert!(tables.contains(&"git_commits".to_string()));
        assert!(tables.contains(&"git_files".to_string()));
        assert!(tables.contains(&"git_blame".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"symbol_relations".to_string()));
        assert!(tables.contains(&"imports".to_string()));
        assert!(tables.contains(&"code_patterns".to_string()));
        assert!(tables.contains(&"code_chunks".to_string()));
        assert!(tables.contains(&"code_embeddings".to_string()));
        assert!(has_table_ish(&db.conn, "code_fts").await);
    }

    #[tokio::test]
    async fn test_insert_and_search_code() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_chunk("chunk1", "src/main.rs", "fn hello() { println!(\"hello\"); }",
            Some("rust"), None, Some(10)).await.unwrap();
        db.conn.execute_batch(&populate_fts_from_chunks_sql()).await.unwrap();
        let results = db.search_code("hello", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/main.rs");
    }

    #[tokio::test]
    async fn test_insert_commit_and_file() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_commit("abc123", "author@example.com", "2026-01-15T10:00:00Z",
            "Initial commit", Some("main"), &[]).await.unwrap();
        db.insert_file("src/lib.rs", Some("2026-01-15"), Some("2026-01-15"),
            120, Some("rust"), Some("crate")).await.unwrap();
        let mut rows = db.conn.query("SELECT COUNT(*) FROM git_commits", ()).await.unwrap();
        let c1: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        let mut rows = db.conn.query("SELECT COUNT(*) FROM git_files", ()).await.unwrap();
        let c2: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(c1, 1);
        assert_eq!(c2, 1);
    }

    #[tokio::test]
    async fn test_insert_symbol_and_relation() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_symbol("sym1", "hello", "function", "src/main.rs",
            10, 20, Some("fn hello()"), Some("pub"), Some("Says hello"),
            "rust", Some("main"), Some(1.0)).await.unwrap();
        db.insert_symbol("sym2", "world", "struct", "src/lib.rs",
            5, 15, None, None, None, "rust", None, None).await.unwrap();
        db.insert_relation("sym1", "sym2", "calls", Some("src/main.rs")).await.unwrap();
        let mut rows = db.conn.query("SELECT COUNT(*) FROM symbols", ()).await.unwrap();
        let c1: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        let mut rows = db.conn.query("SELECT COUNT(*) FROM symbol_relations", ()).await.unwrap();
        let c2: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(c1, 2);
        assert_eq!(c2, 1);
    }

    #[tokio::test]
    async fn test_insert_blame_record() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_commit("abc", "alice", "2026-01-01", "first", Some("main"), &[]).await.unwrap();
        db.insert_blame("src/main.rs", 1, 10, "abc", "alice", "2026-01-01").await.unwrap();
        let mut rows = db.conn.query("SELECT COUNT(*) FROM git_blame", ()).await.unwrap();
        let c: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(c, 1);
    }

    #[tokio::test]
    async fn test_hybrid_search_no_data() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        let embedding = vec![0.0f32; 384];
        let results = db.hybrid_search("nonexistent", &embedding, 10, 60).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_insert_pattern() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_pattern("pat1", "error-handling", "if_let_ok() pattern", "project-a", "auto",
            0.9, Some("src/errors.rs"), 1, Some("if let Ok(v) = result"), "verified").await.unwrap();
        let mut rows = db.conn.query("SELECT COUNT(*) FROM code_patterns", ()).await.unwrap();
        let c: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(c, 1);
    }

    #[tokio::test]
    async fn test_batch_inserts() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();

        let symbols = vec![
            SymbolInput {
                id: "s1".into(), name: "n1".into(), kind: "k1".into(), file_path: "f1".into(),
                line_start: 1, line_end: 2, signature: None, visibility: None,
                doc_comment: None, language: "rust".into(), module_path: None, complexity: None,
            },
            SymbolInput {
                id: "s2".into(), name: "n2".into(), kind: "k2".into(), file_path: "f2".into(),
                line_start: 3, line_end: 4, signature: None, visibility: None,
                doc_comment: None, language: "rust".into(), module_path: None, complexity: None,
            },
        ];
        db.insert_symbols_batch(&symbols).await.unwrap();

        let chunks = vec![
            ChunkInput {
                id: "c1".into(), path: "f1".into(), content: "c1".into(),
                language: None, symbol_id: Some("s1".into()), tokens: None,
            },
            ChunkInput {
                id: "c2".into(), path: "f2".into(), content: "c2".into(),
                language: None, symbol_id: Some("s2".into()), tokens: None,
            },
        ];
        db.insert_chunks_batch(&chunks).await.unwrap();

        let embeddings = vec![
            EmbeddingInput { id: "c1".into(), embedding: vec![0.1; 384] },
            EmbeddingInput { id: "c2".into(), embedding: vec![0.2; 384] },
        ];
        db.insert_embeddings_batch(&embeddings).await.unwrap();

        let mut rows = db.conn.query("SELECT COUNT(*) FROM symbols", ()).await.unwrap();
        assert_eq!(rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(), 2);

        let mut rows = db.conn.query("SELECT COUNT(*) FROM code_chunks", ()).await.unwrap();
        assert_eq!(rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(), 2);

        let mut rows = db.conn.query("SELECT COUNT(*) FROM code_embeddings", ()).await.unwrap();
        assert_eq!(rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(), 2);
    }

    async fn get_table_names(conn: &Connection) -> Vec<String> {
        let mut rows = conn.query("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name", ()).await.unwrap();
        let mut results = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            results.push(row.get(0).unwrap());
        }
        results
    }

    async fn has_table_ish(conn: &Connection, name: &str) -> bool {
        let mut rows = conn.query(
            "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE ?1 AND (type='table' OR type='virtual')",
            params![format!("%{}%", name)],
        ).await.unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        count > 0
    }
}
