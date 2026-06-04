//! Per-project codebase database manager.
//!
//! Creates and manages tables in `.xavier/codebase.db`.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use crate::codebase::connection_manager::ConnectionManager;

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

// ---------------------------------------------------------------------------
// CodebaseDb
// ---------------------------------------------------------------------------

/// Manages the per-project codebase SQLite database.
pub struct CodebaseDb {
    project_id: String,
}

impl CodebaseDb {
    /// Open (or create) the codebase database at `path`.
    pub async fn open(project_root: &Path) -> Result<Self> {
        let project_id = "default"; // Or derive from path
        ConnectionManager::global().connect(project_id, &project_root.to_string_lossy())?;
        Ok(Self { project_id: project_id.to_string() })
    }

    /// Open an in-memory database (for testing).
    pub async fn open_in_memory() -> Result<Self> {
        let project_id = "test_in_memory";
        ConnectionManager::global().connect(project_id, ".")?;
        Ok(Self { project_id: project_id.to_string() })
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
            let sql = format!(
                "SELECT path, content, code_tokens, rank
                 FROM code_fts
                 WHERE code_fts MATCH ?1
                 ORDER BY rank
                 LIMIT ?2"
            );
            let mut stmt = conn.prepare(&sql)?;
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
        ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let sql = format!(
                "SELECT ce.id, cc.path, cc.content, vector_distance_cos(ce.embedding, ?1) as distance
                 FROM code_embeddings ce
                 JOIN code_chunks cc ON cc.id = ce.id
                 ORDER BY distance
                 LIMIT ?2"
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params![embedding_blob, limit as i64])?;
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
