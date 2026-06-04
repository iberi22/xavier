//! Per-project codebase database manager.
//!
//! Creates and manages tables in `.xavier/codebase.db`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use once_cell::sync::OnceCell;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::codebase::connection_manager::ConnectionManager;

/// Global flag ensuring the sqlite-vec extension is registered exactly once.
static SQLITE_VEC_EXTENSION_INIT: OnceCell<Result<(), String>> = OnceCell::new();

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
    pool: Arc<r2d2::Pool<SqliteConnectionManager>>,
}

impl CodebaseDb {
    /// Open (or create) the codebase database for a project.
    pub fn open(project_id: &str, project_root: &str) -> Result<Self> {
        let manager = ConnectionManager::global();
        manager.connect(project_id, project_root)?;
        let pool = manager.get_pool(project_id)?;

        let db = Self { pool };

        // Load extension and verify on a temporary connection
        {
            let conn = db.connection()?;
            Self::register_sqlite_vec_extension()?;
            Self::verify_vec_extension_active(&conn)?;
        }

        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_in_memory() -> Result<Self> {
        let manager = SqliteConnectionManager::memory();
        let pool = r2d2::Pool::new(manager)?;
        let db = Self { pool: Arc::new(pool) };

        {
            let conn = db.connection()?;
            Self::register_sqlite_vec_extension()?;
            Self::verify_vec_extension_active(&conn)?;
        }

        Ok(db)
    }

    /// Create (or migrate) the schema.
    pub fn create_schema(&self) -> Result<()> {
        let conn = self.connection()?;
        let mut stmts = Vec::new();

        // Essential metadata.
        stmts.push(create_repo_meta_table());

        // Git tables.
        stmts.push(create_git_commits_table());
        stmts.push(create_git_blame_table());
        stmts.push(create_git_files_table());

        // Code-analysis tables.
        stmts.push(create_symbols_table());
        stmts.push(create_symbol_relations_table());
        stmts.push(create_imports_table());
        stmts.push(create_patterns_table());

        // code_chunks is needed for both FTS5 and vec0.
        stmts.push(create_code_chunks_table());

        // Virtual tables
        stmts.push(create_code_embeddings_table());
        stmts.push(create_code_fts_table());

        for stmt in &stmts {
            conn.execute_batch(stmt)
                .with_context(|| format!("failed to execute schema SQL:\n{}", stmt))?;
        }
        Ok(())
    }

    /// Return a pooled connection.
    pub fn connection(&self) -> Result<PooledConnection<SqliteConnectionManager>> {
        self.pool.get().context("failed to get connection from pool")
    }

    // ------------------------------------------------------------------
    // Insert helpers
    // ------------------------------------------------------------------

    /// Insert a single git commit record.
    pub fn insert_commit(
        &self, hash: &str, author: &str, date: &str, message: &str,
        branch: Option<&str>, parents: &[&str],
    ) -> Result<()> {
        let parents_json = serde_json::to_string(parents)?;
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO git_commits (hash, author, date, message, branch, parents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![hash, author, date, message, branch, parents_json],
        ).context("failed to insert git commit")?;
        Ok(())
    }

    /// Insert or update a file record.
    pub fn insert_file(
        &self, path: &str, added_at: Option<&str>, last_modified: Option<&str>,
        loc: i64, language: Option<&str>, module_path: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO git_files (path, added_at, last_modified, loc, language, module_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![path, added_at, last_modified, loc, language, module_path],
        ).context("failed to insert file record")?;
        Ok(())
    }

    /// Insert a blame line range.
    pub fn insert_blame(
        &self, file_path: &str, line_start: i64, line_end: i64,
        commit_hash: &str, author: &str, date: &str,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO git_blame (file_path, line_start, line_end, commit_hash, author, date)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![file_path, line_start, line_end, commit_hash, author, date],
        ).context("failed to insert git blame")?;
        Ok(())
    }

    /// Insert a code symbol.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_symbol(
        &self, id: &str, name: &str, kind: &str, file_path: &str,
        line_start: i64, line_end: i64, signature: Option<&str>,
        visibility: Option<&str>, doc_comment: Option<&str>,
        language: &str, module_path: Option<&str>, complexity: Option<f64>,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO symbols
             (id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![id, name, kind, file_path, line_start, line_end, signature, visibility, doc_comment, language, module_path, complexity],
        ).context("failed to insert symbol")?;
        Ok(())
    }

    /// Insert a symbol-to-symbol relation.
    pub fn insert_relation(
        &self, source_id: &str, target_id: &str, relation: &str, file_path: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO symbol_relations (source_id, target_id, relation, file_path)
             VALUES (?1, ?2, ?3, ?4)",
            params![source_id, target_id, relation, file_path],
        ).context("failed to insert symbol relation")?;
        Ok(())
    }

    /// Insert a code chunk with optional embedding and FTS index content.
    pub fn insert_chunk(
        &self, id: &str, path: &str, content: &str,
        language: Option<&str>, symbol_id: Option<&str>, tokens: Option<i64>,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO code_chunks (id, path, content, language, symbol_id, tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, path, content, language, symbol_id, tokens],
        ).context("failed to insert code chunk")?;
        Ok(())
    }

    /// Insert an embedding vector into the vec0 virtual table.
    ///
    /// The `embedding` slice must have exactly 384 elements.
    pub fn insert_embedding(&self, id: &str, embedding: &[f32]) -> Result<()> {
        let embedding_json = serde_json::to_string(embedding)?;
        let conn = self.connection()?;
        conn.execute(
            "INSERT INTO code_embeddings (id, embedding) VALUES (?1, ?2)",
            params![id, embedding_json],
        ).context("failed to insert embedding")?;
        Ok(())
    }

    /// Insert a pattern record.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_pattern(
        &self, id: &str, category: &str, pattern: &str, confidence: f64,
        discovered_by: Option<&str>, source_file: Option<&str>, source_snippet: Option<&str>,
    ) -> Result<()> {
        let conn = self.connection()?;
        conn.execute(
            "INSERT OR REPLACE INTO patterns (id, category, pattern, confidence, discovered_by, source_file, source_snippet)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, category, pattern, confidence, discovered_by, source_file, source_snippet],
        ).context("failed to insert pattern")?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Search helpers
    // ------------------------------------------------------------------

    /// Full-text search over code via the FTS5 virtual table.
    ///
    /// Returns at most `limit` results, ordered by rank (best match first).
    pub fn search_code(&self, query: &str, limit: usize) -> Result<Vec<CodeSearchResult>> {
        let conn = self.connection()?;
        let sql = format!(
            "SELECT c.path, c.content, c.code_tokens, rank
             FROM code_fts
             JOIN code_chunks c ON c.id = code_fts.rowid
             WHERE code_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let results = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(CodeSearchResult {
                    path: row.get(0)?, content: row.get(1)?,
                    code_tokens: row.get(2)?, rank: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to query code_fts")?;
        Ok(results)
    }

    /// Semantic (vector) similarity search via the vec0 virtual table.
    ///
    /// `embedding` must be a slice of 384 f32 values.
    pub fn search_semantic(&self, embedding: &[f32], limit: usize) -> Result<Vec<SemanticSearchResult>> {
        let conn = self.connection()?;
        let embedding_json = serde_json::to_string(embedding)?;
        let sql = format!(
            "SELECT ce.id, cc.path, cc.content, ce.distance
             FROM code_embeddings ce
             JOIN code_chunks cc ON cc.id = ce.id
             WHERE ce.embedding MATCH ?1
             ORDER BY ce.distance
             LIMIT ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        let results = stmt
            .query_map(params![embedding_json, limit as i64], |row| {
                Ok(SemanticSearchResult {
                    id: row.get(0)?, path: row.get(1)?,
                    content: row.get(2)?, distance: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to query code_embeddings")?;
        Ok(results)
    }

    /// Hybrid search combining FTS5 and vector results (RRF-style).
    ///
    /// Returns at most `limit` results sorted by combined relevance.
    /// The FTS and vector searches are run independently and fused
    /// using a simple reciprocal rank.
    pub fn hybrid_search(
        &self, text_query: &str, embedding: &[f32], limit: usize, rrf_k: usize,
    ) -> Result<Vec<HybridResult>> {
        use std::collections::HashMap;
        let mut scores: HashMap<String, HybridResult> = HashMap::new();

        let fts_results = self.search_code(text_query, limit * 2)?;
        for (rank, result) in fts_results.iter().enumerate() {
            let rr = 1.0 / (rrf_k as f64 + rank as f64 + 1.0);
            scores.entry(result.path.clone())
                .and_modify(|e| e.score += rr)
                .or_insert(HybridResult { path: result.path.clone(), content: result.content.clone(), score: rr });
        }

        let vec_results = self.search_semantic(embedding, limit * 2)?;
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

    fn register_sqlite_vec_extension() -> Result<()> {
        SQLITE_VEC_EXTENSION_INIT
            .get_or_init(|| unsafe {
                type SqliteExtFn = unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3, *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32;
                let entry: SqliteExtFn =
                    std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
                let rc = rusqlite::ffi::sqlite3_auto_extension(Some(entry));
                if rc != 0 { Err(format!("failed to register sqlite-vec auto extension: {}", rc)) }
                else { Ok(()) }
            })
            .clone()
            .map_err(anyhow::Error::msg)
    }

    fn verify_vec_extension_active(conn: &Connection) -> Result<()> {
        let is_active: bool = conn
            .query_row(
                "SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE 'sqlite-vec%'",
                [],
                |row| row.get(0),
            ).unwrap_or(false);
        if !is_active {
            let test = conn.execute_batch(
                "CREATE VIRTUAL TABLE IF NOT EXISTS _xavier_vec_check USING vec0(embedding float[1]); DROP TABLE IF EXISTS _xavier_vec_check;",
            );
            if test.is_err() {
                anyhow::bail!("sqlite-vec extension is not active on this connection");
            }
        }
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
    "CREATE TABLE IF NOT EXISTS patterns (id TEXT PRIMARY KEY, category TEXT NOT NULL, pattern TEXT NOT NULL, confidence REAL DEFAULT 0.5, discovered_by TEXT DEFAULT 'auto', source_file TEXT, source_snippet TEXT);".to_string()
}

fn create_code_chunks_table() -> String {
    "CREATE TABLE IF NOT EXISTS code_chunks (id TEXT PRIMARY KEY, path TEXT NOT NULL, content TEXT NOT NULL, embedding BLOB, language TEXT, symbol_id TEXT, tokens INTEGER, FOREIGN KEY (symbol_id) REFERENCES symbols(id));".to_string()
}

fn create_code_embeddings_table() -> String {
    "CREATE VIRTUAL TABLE IF NOT EXISTS code_embeddings USING vec0(embedding float[384], id TEXT);".to_string()
}

fn create_code_fts_table() -> String {
    "CREATE VIRTUAL TABLE IF NOT EXISTS code_fts USING fts5(path, content, code_tokens, tokenize='porter unicode61');".to_string()
}

/// Populate code_fts from code_chunks (used after batch insert).
pub fn populate_fts_from_chunks_sql() -> String {
    "INSERT OR IGNORE INTO code_fts (rowid, path, content, code_tokens) SELECT rowid, path, content, '' FROM code_chunks;".to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        let conn = db.connection().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM repo_meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_create_schema_all_tables() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        let conn = db.connection().unwrap();
        let tables = get_table_names(&conn);
        assert!(tables.contains(&"repo_meta".to_string()));
        assert!(tables.contains(&"git_commits".to_string()));
        assert!(tables.contains(&"git_files".to_string()));
        assert!(tables.contains(&"git_blame".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"symbol_relations".to_string()));
        assert!(tables.contains(&"imports".to_string()));
        assert!(tables.contains(&"patterns".to_string()));
        assert!(tables.contains(&"code_chunks".to_string()));
        assert!(has_table_ish(&conn, "code_embeddings"));
        assert!(has_table_ish(&conn, "code_fts"));
    }

    #[test]
    fn test_insert_and_search_code() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        db.insert_chunk("chunk1", "src/main.rs", "fn hello() { println!(\"hello\"); }",
            Some("rust"), None, Some(10)).unwrap();
        let conn = db.connection().unwrap();
        conn.execute_batch(&populate_fts_from_chunks_sql()).unwrap();
        let results = db.search_code("hello", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "src/main.rs");
    }

    #[test]
    fn test_insert_commit_and_file() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        db.insert_commit("abc123", "author@example.com", "2026-01-15T10:00:00Z",
            "Initial commit", Some("main"), &[]).unwrap();
        db.insert_file("src/lib.rs", Some("2026-01-15"), Some("2026-01-15"),
            120, Some("rust"), Some("crate")).unwrap();
        let conn = db.connection().unwrap();
        let c1: i64 = conn.query_row("SELECT COUNT(*) FROM git_commits", [], |row| row.get(0)).unwrap();
        let c2: i64 = conn.query_row("SELECT COUNT(*) FROM git_files", [], |row| row.get(0)).unwrap();
        assert_eq!(c1, 1);
        assert_eq!(c2, 1);
    }

    #[test]
    fn test_insert_symbol_and_relation() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        db.insert_symbol("sym1", "hello", "function", "src/main.rs",
            10, 20, Some("fn hello()"), Some("pub"), Some("Says hello"),
            "rust", Some("main"), Some(1.0)).unwrap();
        db.insert_symbol("sym2", "world", "struct", "src/lib.rs",
            5, 15, None, None, None, "rust", None, None).unwrap();
        db.insert_relation("sym1", "sym2", "calls", Some("src/main.rs")).unwrap();
        let conn = db.connection().unwrap();
        let c1: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0)).unwrap();
        let c2: i64 = conn.query_row("SELECT COUNT(*) FROM symbol_relations", [], |row| row.get(0)).unwrap();
        assert_eq!(c1, 2);
        assert_eq!(c2, 1);
    }

    #[test]
    fn test_insert_blame_record() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        db.insert_commit("abc", "alice", "2026-01-01", "first", Some("main"), &[]).unwrap();
        db.insert_blame("src/main.rs", 1, 10, "abc", "alice", "2026-01-01").unwrap();
        let conn = db.connection().unwrap();
        let c: i64 = conn.query_row("SELECT COUNT(*) FROM git_blame", [], |row| row.get(0)).unwrap();
        assert_eq!(c, 1);
    }

    #[test]
    fn test_hybrid_search_no_data() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        let embedding = vec![0.0f32; 384];
        let results = db.hybrid_search("nonexistent", &embedding, 10, 60).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_insert_pattern() {
        let db = CodebaseDb::open_in_memory().unwrap();
        db.create_schema().unwrap();
        db.insert_pattern("pat1", "error-handling", "if_let_ok() pattern",
            0.9, Some("auto"), Some("src/errors.rs"), Some("if let Ok(v) = result")).unwrap();
        let conn = db.connection().unwrap();
        let c: i64 = conn.query_row("SELECT COUNT(*) FROM patterns", [], |row| row.get(0)).unwrap();
        assert_eq!(c, 1);
    }

    fn get_table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").unwrap();
        stmt.query_map([], |row| row.get(0)).unwrap().filter_map(|r| r.ok()).collect()
    }

    fn has_table_ish(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE ?1 AND (type='table' OR type='virtual')",
            params![format!("%{}%", name)],
            |row| row.get(0),
        ).unwrap_or(0);
        count > 0
    }
}
