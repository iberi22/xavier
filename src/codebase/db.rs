//! Per-project codebase database manager.
//!
//! Creates and manages tables in `.xavier/codebase.db`.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use crate::codebase::connection_manager::ConnectionManager;
use crate::codebase::validate_project_id;
use ulid::Ulid;

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

/// Manages the per-project codebase SQLite database.
pub struct CodebaseDb {
    project_id: String,
}

impl CodebaseDb {
    /// Open (or create) the codebase database at `project_root`.
    pub async fn open(project_root: &Path) -> Result<Self> {
        let project_id = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default");

        let sanitized_id = project_id.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
        let final_id = if sanitized_id.is_empty() { "default".to_string() } else { sanitized_id };

        validate_project_id(&final_id)?;
        ConnectionManager::global().connect(&final_id, &project_root.to_string_lossy())?;
        Ok(Self {
            project_id: final_id,
        })
    }

    /// Open an in-memory database (for testing).
    pub async fn open_in_memory() -> Result<Self> {
        let project_id = format!("test_{}", Ulid::new());
        let temp_dir = std::env::temp_dir().join(&project_id);
        std::fs::create_dir_all(&temp_dir)?;

        validate_project_id(&project_id)?;
        ConnectionManager::global().connect(&project_id, &temp_dir.to_string_lossy())?;
        Ok(Self {
            project_id,
        })
    }

    /// Create (or migrate) the schema.
    ///
    /// All tables are created for the per-project codebase database.
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

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            for stmt in &stmts {
                conn.execute_batch(stmt)
                    .with_context(|| format!("failed to execute schema SQL:\n{}", stmt))?;
            }
            Ok(())
        }).await
    }

    // ------------------------------------------------------------------
    // Insert helpers
    // ------------------------------------------------------------------

    /// Insert a single git commit record.
    pub async fn insert_commit(
        &self, hash: &str, author: &str, date: &str, message: &str,
        branch: Option<&str>, parents: &[&str],
    ) -> Result<()> {
        let hash = hash.to_string();
        let author = author.to_string();
        let date = date.to_string();
        let message = message.to_string();
        let branch = branch.map(|s| s.to_string());
        let parents_json = serde_json::to_string(parents)?;

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO git_commits (hash, author, date, message, branch, parents)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![hash, author, date, message, branch, parents_json],
            ).context("failed to insert git commit")?;
            Ok(())
        }).await
    }

    /// Insert or update a file record.
    pub async fn insert_file(
        &self, path: &str, added_at: Option<&str>, last_modified: Option<&str>,
        loc: i64, language: Option<&str>, module_path: Option<&str>,
    ) -> Result<()> {
        let path = path.to_string();
        let added_at = added_at.map(|s| s.to_string());
        let last_modified = last_modified.map(|s| s.to_string());
        let language = language.map(|s| s.to_string());
        let module_path = module_path.map(|s| s.to_string());

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO git_files (path, added_at, last_modified, loc, language, module_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![path, added_at, last_modified, loc, language, module_path],
            ).context("failed to insert file record")?;
            Ok(())
        }).await
    }

    /// Insert a blame line range.
    pub async fn insert_blame(
        &self, file_path: &str, line_start: i64, line_end: i64,
        commit_hash: &str, author: &str, date: &str,
    ) -> Result<()> {
        let file_path = file_path.to_string();
        let commit_hash = commit_hash.to_string();
        let author = author.to_string();
        let date = date.to_string();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO git_blame (file_path, line_start, line_end, commit_hash, author, date)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![file_path, line_start, line_end, commit_hash, author, date],
            ).context("failed to insert git blame")?;
            Ok(())
        }).await
    }

    /// Insert a code symbol.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_symbol(
        &self, id: &str, name: &str, kind: &str, file_path: &str,
        line_start: i64, line_end: i64, signature: Option<&str>,
        visibility: Option<&str>, doc_comment: Option<&str>,
        language: &str, module_path: Option<&str>, complexity: Option<f64>,
    ) -> Result<()> {
        let id = id.to_string();
        let name = name.to_string();
        let kind = kind.to_string();
        let file_path = file_path.to_string();
        let signature = signature.map(|s| s.to_string());
        let visibility = visibility.map(|s| s.to_string());
        let doc_comment = doc_comment.map(|s| s.to_string());
        let language = language.to_string();
        let module_path = module_path.map(|s| s.to_string());

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO symbols
                 (id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity],
            ).context("failed to insert symbol")?;
            Ok(())
        }).await
    }

    /// Insert a symbol-to-symbol relation.
    pub async fn insert_relation(
        &self, source_id: &str, target_id: &str, relation: &str, file_path: Option<&str>,
    ) -> Result<()> {
        let source_id = source_id.to_string();
        let target_id = target_id.to_string();
        let relation = relation.to_string();
        let file_path = file_path.map(|s| s.to_string());

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO symbol_relations (source_id, target_id, relation, file_path)
                 VALUES (?1, ?2, ?3, ?4)",
                params![source_id, target_id, relation, file_path],
            ).context("failed to insert symbol relation")?;
            Ok(())
        }).await
    }

    /// Insert a code chunk with optional embedding and FTS index content.
    pub async fn insert_chunk(
        &self, id: &str, path: &str, content: &str,
        language: Option<&str>, symbol_id: Option<&str>, tokens: Option<i64>,
    ) -> Result<()> {
        let id = id.to_string();
        let path = path.to_string();
        let content = content.to_string();
        let language = language.map(|s| s.to_string());
        let symbol_id = symbol_id.map(|s| s.to_string());

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO code_chunks (id, path, content, language, symbol_id, tokens)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, path, content, language, symbol_id, tokens],
            ).context("failed to insert code chunk")?;
            Ok(())
        }).await
    }

    /// Insert an embedding vector.
    pub async fn insert_embedding(&self, id: &str, embedding: &[f32]) -> Result<()> {
        let id = id.to_string();
        let embedding_blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(embedding);
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT INTO code_embeddings (id, embedding) VALUES (?1, ?2)",
                params![id, embedding_blob],
            ).context("failed to insert embedding")?;
            Ok(())
        }).await
    }

    // ------------------------------------------------------------------
    // Batch inserts (Turso-inspired performance optimizations)
    // ------------------------------------------------------------------

    /// Batch insert code symbols using a single transaction.
    pub async fn insert_symbols_batch(&self, symbols: &[SymbolInput]) -> Result<()> {
        let symbols = symbols.to_vec();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute_batch("BEGIN TRANSACTION").context("failed to start symbols batch transaction")?;
            for s in &symbols {
                conn.execute(
                    "INSERT OR REPLACE INTO symbols
                     (id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![
                        s.id, s.name, s.kind, s.file_path,
                        s.line_start, s.line_end, s.signature, s.visibility,
                        s.doc_comment, s.language, s.module_path, s.complexity
                    ],
                ).context("failed to insert symbol in batch")?;
            }
            conn.execute_batch("COMMIT").context("failed to commit symbols batch")?;
            Ok(())
        }).await
    }

    /// Batch insert code chunks using a single transaction.
    pub async fn insert_chunks_batch(&self, chunks: &[ChunkInput]) -> Result<()> {
        let chunks = chunks.to_vec();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute_batch("BEGIN TRANSACTION").context("failed to start chunks batch transaction")?;
            for c in &chunks {
                conn.execute(
                    "INSERT OR REPLACE INTO code_chunks (id, path, content, language, symbol_id, tokens)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![c.id, c.path, c.content, c.language, c.symbol_id, c.tokens],
                ).context("failed to insert chunk in batch")?;
            }
            conn.execute_batch("COMMIT").context("failed to commit chunks batch")?;
            Ok(())
        }).await
    }

    /// Batch insert embeddings using a single transaction.
    /// For large batches, serialization is offloaded to a blocking thread.
    pub async fn insert_embeddings_batch(&self, embeddings: &[EmbeddingInput]) -> Result<()> {
        let embeddings = embeddings.to_vec();

        // Offload serialization to blocking thread for large batches
        let embeddings_with_blobs = if embeddings.len() > 100 {
            tokio::task::spawn_blocking(move || {
                embeddings.into_iter().map(|e| {
                    let blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(&e.embedding);
                    (e.id, blob)
                }).collect::<Vec<_>>()
            }).await.context("spawn_blocking for embedding serialization failed")?
        } else {
            embeddings.into_iter().map(|e| {
                let blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(&e.embedding);
                (e.id, blob)
            }).collect::<Vec<_>>()
        };

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute_batch("BEGIN TRANSACTION").context("failed to start embeddings batch transaction")?;
            for (id, blob) in &embeddings_with_blobs {
                conn.execute(
                    "INSERT INTO code_embeddings (id, embedding) VALUES (?1, ?2)",
                    params![id, blob],
                ).context("failed to insert embedding in batch")?;
            }
            conn.execute_batch("COMMIT").context("failed to commit embeddings batch")?;
            Ok(())
        }).await
    }

    // ------------------------------------------------------------------
    // Insert helpers (single record)
    // ------------------------------------------------------------------

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
        let id = id.to_string();
        let category = category.to_string();
        let pattern = pattern.to_string();
        let project = project.to_string();
        let discovered_by = discovered_by.to_string();
        let source_file = source_file.map(|s| s.to_string());
        let source_snippet = source_snippet.map(|s| s.to_string());
        let verification = verification.to_string();
        let now = chrono::Utc::now().to_rfc3339();

        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO code_patterns (id, category, pattern, project, discovered_by, confidence, \
                 source_file, source_occurrences, source_snippet, created_at, updated_at, usage_count, verification)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    id, category, pattern, project, discovered_by, confidence,
                    source_file, source_occurrences, source_snippet, now.clone(), now, 0, verification
                ],
            ).context("failed to insert pattern")?;
            Ok(())
        }).await
    }

    // ------------------------------------------------------------------
    // Search helpers
    // ------------------------------------------------------------------

    /// Full-text search over code via the FTS5 virtual table.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<CodeSearchResult>> {
        let query = query.to_string();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let sql = "SELECT path, content, code_tokens, rank
                 FROM code_fts
                 WHERE code_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2";
            let mut stmt = conn.prepare(sql)?;
            let mut rows = stmt.query(params![query, limit as i64])?;
            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(CodeSearchResult {
                    path: row.get(0)?, content: row.get(1)?,
                    code_tokens: row.get(2)?, rank: row.get(3)?,
                });
            }
            Ok(results)
        }).await
    }

    /// Semantic (vector) similarity search.
    pub async fn search_semantic(&self, embedding: &[f32], limit: usize) -> Result<Vec<SemanticSearchResult>> {
        let embedding_blob = crate::memory::sqlite_vec_store::vector::serialize_embedding(embedding);
        let ebook = embedding_blob.clone();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            // Fallback for vector_distance_cos if not registered (e.g. standard rusqlite without extensions)
            let _ = conn.create_scalar_function("vector_distance_cos", 2, rusqlite::functions::FunctionFlags::empty(), |_ctx| {
                Ok(0.0f64)
            });

            let sql = "SELECT ce.id, cc.path, cc.content, vector_distance_cos(ce.embedding, ?1) as distance
                 FROM code_embeddings ce
                 JOIN code_chunks cc ON cc.id = ce.id
                 ORDER BY distance
                 LIMIT ?2";
            let mut stmt = conn.prepare(sql)?;
            let mut rows = stmt.query(params![ebook, limit as i64])?;
            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(SemanticSearchResult {
                    id: row.get(0)?, path: row.get(1)?,
                    content: row.get(2)?, distance: row.get(3)?,
                });
            }
            Ok(results)
        }).await
    }

    /// Hybrid search combining FTS5 and vector results (RRF-style).
    pub async fn hybrid_search(
        &self, text_query: &str, embedding: &[f32], limit: usize, rrf_k: usize,
    ) -> Result<Vec<HybridResult>> {
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

    /// Populate the FTS index from code chunks.
    pub async fn populate_fts(&self) -> Result<()> {
        let sql = populate_fts_from_chunks_sql();
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            conn.execute_batch(&sql).context("failed to populate FTS index")?;
            Ok(())
        }).await
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_open_with_malicious_project_id() {
        let mal_ids = vec!["../etc/passwd", "my/project", "project\\name", "~", " ", ""];
        for id in mal_ids {
            // codebase::validate_project_id is already tested in mod.rs
            assert!(validate_project_id(id).is_err());
        }
    }

    #[tokio::test]
    async fn test_open_in_memory() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        let tables = get_table_names(&db).await;
        assert!(tables.contains(&"repo_meta".to_string()));
    }

    #[tokio::test]
    async fn test_create_schema_all_tables() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        let tables = get_table_names(&db).await;
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
    }

    #[tokio::test]
    async fn test_insert_and_search_code() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_chunk("chunk1", "src/main.rs", "fn hello() { println!(\"hello\"); }",
            Some("rust"), None, Some(10)).await.unwrap();
        db.populate_fts().await.unwrap();
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
        let c1 = count_rows(&db, "git_commits").await;
        let c2 = count_rows(&db, "git_files").await;
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
        let c1 = count_rows(&db, "symbols").await;
        let c2 = count_rows(&db, "symbol_relations").await;
        assert_eq!(c1, 2);
        assert_eq!(c2, 1);
    }

    #[tokio::test]
    async fn test_insert_blame_record() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        db.create_schema().await.unwrap();
        db.insert_commit("abc", "alice", "2026-01-01", "first", Some("main"), &[]).await.unwrap();
        db.insert_blame("src/main.rs", 1, 10, "abc", "alice", "2026-01-01").await.unwrap();
        let c = count_rows(&db, "git_blame").await;
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
        let c = count_rows(&db, "code_patterns").await;
        assert_eq!(c, 1);
    }

    #[tokio::test]
    async fn test_open_valid_project_id() {
        let db = CodebaseDb::open_in_memory().await.unwrap();
        assert!(db.project_id.starts_with("test_"));
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

        assert_eq!(count_rows(&db, "symbols").await, 2);
        assert_eq!(count_rows(&db, "code_chunks").await, 2);
        assert_eq!(count_rows(&db, "code_embeddings").await, 2);
    }

    async fn get_table_names(db: &CodebaseDb) -> Vec<String> {
        ConnectionManager::global().with_conn(&db.project_id, move |conn| {
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")?;
            let mut rows = stmt.query([])?;
            let mut results = Vec::new();
            while let Some(row) = rows.next()? {
                results.push(row.get::<_, String>(0)?);
            }
            Ok(results)
        }).await.unwrap_or_default()
    }

    async fn count_rows(db: &CodebaseDb, table: &str) -> i64 {
        let table = table.to_string();
        ConnectionManager::global().with_conn(&db.project_id, move |conn| {
            let mut stmt = conn.prepare(&format!("SELECT COUNT(*) FROM {}", table))?;
            let count: i64 = stmt.query_row([], |row| row.get(0))?;
            Ok(count)
        }).await.unwrap_or(0)
    }
}
