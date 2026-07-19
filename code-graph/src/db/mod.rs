//! SQLite database for storing code graph
// Build force-recompile 2026-05-27

pub mod benchmarks;

use crate::error::{GraphError, Result};
use crate::types::{
    CodeEdge, ComplexityHotspot, EdgeType, HubNode, IndexStats, Language, LanguageCount,
    QueryResult, Symbol, SymbolKind,
};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;
use tracing::{debug, info};

const DEFAULT_PROJECT_ID: &str = "default";

pub struct CodeGraphDB {
    conn: Mutex<Connection>,
}

fn parse_language(value: &str) -> Language {
    serde_json::from_str(value).unwrap_or(match value {
        "Rust" => Language::Rust,
        "TypeScript" => Language::TypeScript,
        "JavaScript" => Language::JavaScript,
        "Python" => Language::Python,
        "Go" => Language::Go,
        "Java" => Language::Java,
        "C" => Language::C,
        "Cpp" => Language::Cpp,
        _ => Language::Unknown,
    })
}

fn parse_symbol_kind(value: &str) -> SymbolKind {
    serde_json::from_str(value).unwrap_or(match value {
        "Function" => SymbolKind::Function,
        "Struct" => SymbolKind::Struct,
        "Enum" => SymbolKind::Enum,
        "Trait" => SymbolKind::Trait,
        "Impl" => SymbolKind::Impl,
        "Class" => SymbolKind::Class,
        "Method" => SymbolKind::Method,
        "Variable" => SymbolKind::Variable,
        "Constant" => SymbolKind::Constant,
        "Import" => SymbolKind::Import,
        "Export" => SymbolKind::Export,
        "Module" => SymbolKind::Module,
        "File" => SymbolKind::File,
        _ => SymbolKind::Symbol,
    })
}

fn parse_edge_type(value: &str) -> EdgeType {
    serde_json::from_str(value).unwrap_or(match value {
        "Calls" => EdgeType::Calls,
        "Defines" => EdgeType::Defines,
        "Uses" => EdgeType::Uses,
        "Imports" => EdgeType::Imports,
        "Contains" => EdgeType::Contains,
        _ => EdgeType::References,
    })
}

fn normalize_symbol_for_insert(symbol: &Symbol) -> Symbol {
    let mut symbol = symbol.clone();
    if symbol.stable_id.as_deref().unwrap_or_default().is_empty() {
        symbol.stable_id = Some(symbol.deterministic_id(DEFAULT_PROJECT_ID));
    }
    symbol
}

fn edge_type_filter(edge_type: Option<EdgeType>) -> Option<&'static str> {
    edge_type.map(|kind| kind.as_str())
}

fn edge_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CodeEdge> {
    let metadata: Option<String> = row.get(7)?;
    Ok(CodeEdge {
        id: Some(row.get(0)?),
        from_symbol: row.get(1)?,
        to_symbol: row.get(2)?,
        edge_type: parse_edge_type(&row.get::<_, String>(3)?),
        file_path: row.get(4)?,
        line: row.get(5)?,
        confidence: row.get(6)?,
        metadata: metadata.and_then(|value| serde_json::from_str(&value).ok()),
    })
}

impl CodeGraphDB {
    /// Open or create a database at the given path
    pub fn new(path: &Path) -> Result<Self> {
        info!("Opening database at {:?}", path);

        let conn = Connection::open(path).map_err(|e| GraphError::Database(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.init_schema()?;
        Ok(db)
    }

    /// Create a new database (overwrite if exists)
    pub fn create_new(path: &Path) -> Result<Self> {
        info!("Creating NEW database at {:?}", path);

        // Remove existing file if present
        if path.exists() {
            std::fs::remove_file(path).map_err(|e| GraphError::Database(e.to_string()))?;
        }

        let conn = Connection::open(path).map_err(|e| GraphError::Database(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.init_schema()?;
        Ok(db)
    }

    /// Create an in-memory database
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| GraphError::Database(e.to_string()))?;

        let db = Self {
            conn: Mutex::new(conn),
        };

        db.init_schema()?;
        Ok(db)
    }

    /// Initialize database schema
    fn init_schema(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                stable_id TEXT NOT NULL DEFAULT '',
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                lang TEXT NOT NULL,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                start_col INTEGER NOT NULL,
                end_col INTEGER NOT NULL,
                signature TEXT,
                parent TEXT,
                complexity REAL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);
            CREATE INDEX IF NOT EXISTS idx_symbols_lang ON symbols(lang);
            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_path);

            CREATE TABLE IF NOT EXISTS refs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                symbol_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                line INTEGER NOT NULL,
                col INTEGER NOT NULL,
                context TEXT,
                FOREIGN KEY (symbol_id) REFERENCES symbols(id)
            );

            CREATE INDEX IF NOT EXISTS idx_refs_symbol ON refs(symbol_id);

            CREATE TABLE IF NOT EXISTS imports (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_path TEXT NOT NULL,
                to_path TEXT NOT NULL,
                file_path TEXT NOT NULL,
                line INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_path);

            CREATE TABLE IF NOT EXISTS edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_symbol TEXT NOT NULL,
                to_symbol TEXT NOT NULL,
                edge_type TEXT NOT NULL,
                file_path TEXT NOT NULL,
                line INTEGER NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                metadata TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(from_symbol, to_symbol, edge_type, file_path, line)
            );

            CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_symbol);
            CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_symbol);
            CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);
            CREATE INDEX IF NOT EXISTS idx_edges_file ON edges(file_path);

            CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_metadata (
                path TEXT PRIMARY KEY,
                mtime INTEGER NOT NULL
            );
            "#,
        )
        .map_err(|e| GraphError::Database(e.to_string()))?;

        drop(conn);
        self.ensure_column("symbols", "stable_id", "TEXT NOT NULL DEFAULT ''")?;
        self.ensure_column("symbols", "signature", "TEXT")?;
        self.ensure_column("symbols", "parent", "TEXT")?;
        self.ensure_column("symbols", "complexity", "REAL")?;

        // Create indexes that might depend on added columns
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
            conn.execute_batch(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_symbols_stable_id ON symbols(stable_id);",
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;
        }

        // Initialize FTS5 virtual table for symbols
        {
            let conn = self
                .conn
                .lock()
                .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
            conn.execute_batch(
                r#"
                CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                    name,
                    stable_id UNINDEXED,
                    file_path UNINDEXED
                );

                -- Migrate existing data into symbols_fts if symbols_fts is empty or has missing symbols
                INSERT INTO symbols_fts (name, stable_id, file_path)
                SELECT s.name, s.stable_id, s.file_path FROM symbols s
                LEFT JOIN symbols_fts f ON s.stable_id = f.stable_id
                WHERE f.stable_id IS NULL;
                "#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;
        }

        info!("Database schema initialized");
        Ok(())
    }

    fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .map_err(|e| GraphError::Database(e.to_string()))?;
        let exists = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|value| value.ok())
            .any(|name| name == column);
        drop(stmt);

        if !exists {
            conn.execute_batch(&format!(
                "ALTER TABLE {} ADD COLUMN {} {};",
                table, column, definition
            ))
            .map_err(|e| GraphError::Database(e.to_string()))?;
        }
        Ok(())
    }

    /// Insert a symbol
    pub fn insert_symbol(&self, symbol: &Symbol) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let symbol = normalize_symbol_for_insert(symbol);
        let stable_id = symbol.stable_id.clone().unwrap_or_default();

        conn.execute(
            r#"INSERT INTO symbols (stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
               ON CONFLICT(stable_id) DO UPDATE SET
                 name=excluded.name,
                 kind=excluded.kind,
                 lang=excluded.lang,
                 file_path=excluded.file_path,
                 start_line=excluded.start_line,
                 end_line=excluded.end_line,
                 start_col=excluded.start_col,
                 end_col=excluded.end_col,
                 signature=excluded.signature,
                 parent=excluded.parent,
                 complexity=excluded.complexity"#,
            params![
                &stable_id,
                symbol.name,
                format!("{:?}", symbol.kind),
                format!("{:?}", symbol.lang),
                symbol.file_path,
                symbol.start_line,
                symbol.end_line,
                symbol.start_col,
                symbol.end_col,
                symbol.signature,
                symbol.parent,
                symbol.complexity,
            ],
        )
        .map_err(|e| GraphError::Database(e.to_string()))?;

        // Sync FTS5 virtual table
        conn.execute(
            "DELETE FROM symbols_fts WHERE stable_id = ?1",
            params![&stable_id],
        )
        .map_err(|e| GraphError::Database(e.to_string()))?;

        conn.execute(
            "INSERT INTO symbols_fts (name, stable_id, file_path) VALUES (?1, ?2, ?3)",
            params![&symbol.name, &stable_id, &symbol.file_path],
        )
        .map_err(|e| GraphError::Database(e.to_string()))?;

        conn.query_row(
            "SELECT id FROM symbols WHERE stable_id = ?1",
            params![&stable_id],
            |row| row.get(0),
        )
        .map_err(|e| GraphError::Database(e.to_string()))
    }

    /// Insert multiple symbols in a batch
    pub fn insert_symbols(&self, symbols: &[Symbol]) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let tx = conn
            .transaction()
            .map_err(|e| GraphError::Database(e.to_string()))?;

        {
            let mut stmt = tx
                .prepare(
                    r#"INSERT INTO symbols (stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                       ON CONFLICT(stable_id) DO UPDATE SET
                         name=excluded.name,
                         kind=excluded.kind,
                         lang=excluded.lang,
                         file_path=excluded.file_path,
                         start_line=excluded.start_line,
                         end_line=excluded.end_line,
                         start_col=excluded.start_col,
                         end_col=excluded.end_col,
                         signature=excluded.signature,
                         parent=excluded.parent,
                         complexity=excluded.complexity"#,
                )
                .map_err(|e| GraphError::Database(e.to_string()))?;

            let mut delete_fts_stmt = tx
                .prepare("DELETE FROM symbols_fts WHERE stable_id = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;

            let mut insert_fts_stmt = tx
                .prepare("INSERT INTO symbols_fts (name, stable_id, file_path) VALUES (?1, ?2, ?3)")
                .map_err(|e| GraphError::Database(e.to_string()))?;

            for symbol in symbols {
                let symbol = normalize_symbol_for_insert(symbol);
                let stable_id = symbol.stable_id.clone().unwrap_or_default();
                stmt.execute(params![
                    &stable_id,
                    symbol.name,
                    format!("{:?}", symbol.kind),
                    format!("{:?}", symbol.lang),
                    symbol.file_path,
                    symbol.start_line,
                    symbol.end_line,
                    symbol.start_col,
                    symbol.end_col,
                    symbol.signature,
                    symbol.parent,
                    symbol.complexity,
                ])
                .map_err(|e| GraphError::Database(e.to_string()))?;

                delete_fts_stmt
                    .execute(params![&stable_id])
                    .map_err(|e| GraphError::Database(e.to_string()))?;

                insert_fts_stmt
                    .execute(params![&symbol.name, &stable_id, &symbol.file_path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
            }
        }

        tx.commit()
            .map_err(|e| GraphError::Database(e.to_string()))?;

        debug!("Inserted {} symbols", symbols.len());
        Ok(())
    }

    /// Calculate search score for ranking results
    /// exact = 10, prefix = 5, fuzzy = 1, bonus for public/exports
    fn calculate_score(symbol_name: &str, query: &str) -> i32 {
        let name_lower = symbol_name.to_lowercase();
        let query_lower = query.to_lowercase();

        // Exact match (case insensitive)
        if name_lower == query_lower {
            return 10;
        }

        // Prefix match
        if name_lower.starts_with(&query_lower) {
            return 5;
        }

        // Contains match
        if name_lower.contains(&query_lower) {
            return 1;
        }

        // Fuzzy - check if all chars exist in order
        let mut query_chars = query_lower.chars().peekable();
        for c in name_lower.chars() {
            if query_chars.peek() == Some(&c) {
                query_chars.next();
            }
        }
        if query_chars.peek().is_none() {
            return 1;
        }

        0
    }

    /// Find symbols by name with hybrid ranking
    pub fn find_symbols(&self, query: &str, limit: usize) -> Result<QueryResult> {
        let start = std::time::Instant::now();
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let mut symbols: Vec<Symbol>;

        if query.is_empty() {
            let mut stmt = conn
                .prepare(
                    r#"SELECT id, stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity
                       FROM symbols
                       WHERE name LIKE ?1"#,
                )
                .map_err(|e| GraphError::Database(e.to_string()))?;

            let pattern = format!("%{}%", query);
            symbols = stmt
                .query_map(params![pattern], |row| {
                    Ok(Symbol {
                        id: Some(row.get(0)?),
                        stable_id: Some(row.get(1)?),
                        name: row.get(2)?,
                        kind: parse_symbol_kind(&row.get::<_, String>(3)?),
                        lang: parse_language(&row.get::<_, String>(4)?),
                        file_path: row.get(5)?,
                        start_line: row.get(6)?,
                        end_line: row.get(7)?,
                        start_col: row.get(8)?,
                        end_col: row.get(9)?,
                        signature: row.get(10)?,
                        parent: row.get(11)?,
                        complexity: row.get(12)?,
                    })
                })
                .map_err(|e| GraphError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
        } else {
            // MATCH query with BM25 rank
            let mut stmt = conn
                .prepare(
                    r#"SELECT s.id, s.stable_id, s.name, s.kind, s.lang, s.file_path, s.start_line, s.end_line, s.start_col, s.end_col, s.signature, s.parent, s.complexity
                       FROM symbols s
                       JOIN symbols_fts ON s.stable_id = symbols_fts.stable_id
                       WHERE symbols_fts MATCH ?1
                       ORDER BY bm25(symbols_fts)
                       LIMIT ?2"#,
                )
                .map_err(|e| GraphError::Database(e.to_string()))?;

            let fts_query = format!("\"{}\"*", query.replace('"', "\"\""));
            symbols = stmt
                .query_map(params![fts_query, limit as isize], |row| {
                    Ok(Symbol {
                        id: Some(row.get(0)?),
                        stable_id: Some(row.get(1)?),
                        name: row.get(2)?,
                        kind: parse_symbol_kind(&row.get::<_, String>(3)?),
                        lang: parse_language(&row.get::<_, String>(4)?),
                        file_path: row.get(5)?,
                        start_line: row.get(6)?,
                        end_line: row.get(7)?,
                        start_col: row.get(8)?,
                        end_col: row.get(9)?,
                        signature: row.get(10)?,
                        parent: row.get(11)?,
                        complexity: row.get(12)?,
                    })
                })
                .map_err(|e| GraphError::Database(e.to_string()))?
                .filter_map(|r| r.ok())
                .collect();
        }

        // Apply scoring and ranking without mutating semantic fields.
        if !query.is_empty() {
            symbols.sort_by(|a, b| {
                let score_for = |symbol: &Symbol| {
                    let score = Self::calculate_score(&symbol.name, query);
                    let bonus = match symbol.kind {
                        SymbolKind::Function | SymbolKind::Struct => 1,
                        _ => 0,
                    };
                    score + bonus
                };
                score_for(b).cmp(&score_for(a))
            });
        }

        // Apply limit
        symbols.truncate(limit);

        let total = symbols.len();
        let query_time_ms = start.elapsed().as_millis() as u64;

        Ok(QueryResult {
            symbols,
            total,
            query_time_ms,
        })
    }

    /// Find symbols in a specific file
    pub fn find_by_file(&self, file_path: &str) -> Result<Vec<Symbol>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let mut stmt = conn
            .prepare(
                r#"SELECT id, stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity
                   FROM symbols
                   WHERE file_path = ?1"#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let symbols = stmt
            .query_map(params![file_path], |row| {
                Ok(Symbol {
                    id: Some(row.get(0)?),
                    stable_id: Some(row.get(1)?),
                    name: row.get(2)?,
                    kind: parse_symbol_kind(&row.get::<_, String>(3)?),
                    lang: parse_language(&row.get::<_, String>(4)?),
                    file_path: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    start_col: row.get(8)?,
                    end_col: row.get(9)?,
                    signature: row.get(10)?,
                    parent: row.get(11)?,
                    complexity: row.get(12)?,
                })
            })
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(symbols)
    }

    /// Find symbols by kind
    pub fn find_by_kind(&self, kind: SymbolKind, limit: usize) -> Result<Vec<Symbol>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let mut stmt = conn
            .prepare(
                r#"SELECT id, stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity
                   FROM symbols
                   WHERE kind = ?1
                   LIMIT ?2"#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let kind_str = format!("{:?}", kind);
        let symbols = stmt
            .query_map(params![kind_str, limit as isize], |row| {
                Ok(Symbol {
                    id: Some(row.get(0)?),
                    stable_id: Some(row.get(1)?),
                    name: row.get(2)?,
                    kind: parse_symbol_kind(&row.get::<_, String>(3)?),
                    lang: parse_language(&row.get::<_, String>(4)?),
                    file_path: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    start_col: row.get(8)?,
                    end_col: row.get(9)?,
                    signature: row.get(10)?,
                    parent: row.get(11)?,
                    complexity: row.get(12)?,
                })
            })
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(symbols)
    }

    /// Get statistics
    pub fn stats(&self) -> Result<IndexStats> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let total_files: u64 = conn
            .query_row("SELECT COUNT(DISTINCT file_path) FROM symbols", [], |row| {
                row.get::<_, i64>(0).map(|v| v as u64)
            })
            .unwrap_or(0);

        let total_symbols: u64 = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0).map(|v| v as u64)
            })
            .unwrap_or(0);

        let total_imports: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE edge_type = 'Imports'",
                [],
                |row| row.get::<_, i64>(0).map(|v| v as u64),
            )
            .unwrap_or(0);

        let mut stmt = conn
            .prepare("SELECT lang, COUNT(*) FROM symbols GROUP BY lang")
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let languages = stmt
            .query_map([], |row| {
                let lang_str: String = row.get(0)?;
                let count: u64 = row.get::<_, i64>(1)? as u64;
                Ok(LanguageCount {
                    lang: parse_language(&lang_str),
                    count,
                })
            })
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(IndexStats {
            total_files,
            total_symbols,
            total_imports,
            languages,
            duration_ms: 0,
        })
    }

    pub fn insert_edge(&self, edge: &CodeEdge) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let metadata = edge.metadata.as_ref().map(|value| value.to_string());
        conn.execute(
            r#"INSERT OR IGNORE INTO edges (from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            params![
                edge.from_symbol,
                edge.to_symbol,
                edge.edge_type.as_str(),
                edge.file_path,
                edge.line,
                edge.confidence,
                metadata,
            ],
        )
        .map_err(|e| GraphError::Database(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    pub fn insert_edges(&self, edges: &[CodeEdge]) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let tx = conn
            .transaction()
            .map_err(|e| GraphError::Database(e.to_string()))?;
        {
            let mut stmt = tx
                .prepare(
                    r#"INSERT OR IGNORE INTO edges (from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
                )
                .map_err(|e| GraphError::Database(e.to_string()))?;

            for edge in edges {
                let metadata = edge.metadata.as_ref().map(|value| value.to_string());
                stmt.execute(params![
                    edge.from_symbol,
                    edge.to_symbol,
                    edge.edge_type.as_str(),
                    edge.file_path,
                    edge.line,
                    edge.confidence,
                    metadata,
                ])
                .map_err(|e| GraphError::Database(e.to_string()))?;
            }
        }
        tx.commit()
            .map_err(|e| GraphError::Database(e.to_string()))?;
        Ok(())
    }

    pub fn find_edges_from(
        &self,
        from_symbol: &str,
        edge_type: Option<EdgeType>,
        limit: usize,
    ) -> Result<Vec<CodeEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let edge_type = edge_type_filter(edge_type);
        let sql = if edge_type.is_some() {
            r#"SELECT id, from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata
               FROM edges WHERE from_symbol = ?1 AND edge_type = ?2 LIMIT ?3"#
        } else {
            r#"SELECT id, from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata
               FROM edges WHERE from_symbol = ?1 LIMIT ?2"#
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| GraphError::Database(e.to_string()))?;
        let rows = if let Some(edge_type) = edge_type {
            stmt.query_map(
                params![from_symbol, edge_type, limit as isize],
                edge_from_row,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?
        } else {
            stmt.query_map(params![from_symbol, limit as isize], edge_from_row)
                .map_err(|e| GraphError::Database(e.to_string()))?
        };
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn find_edges_to(
        &self,
        to_symbol: &str,
        edge_type: Option<EdgeType>,
        limit: usize,
    ) -> Result<Vec<CodeEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let edge_type = edge_type_filter(edge_type);
        let sql = if edge_type.is_some() {
            r#"SELECT id, from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata
               FROM edges WHERE to_symbol = ?1 AND edge_type = ?2 LIMIT ?3"#
        } else {
            r#"SELECT id, from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata
               FROM edges WHERE to_symbol = ?1 LIMIT ?2"#
        };
        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| GraphError::Database(e.to_string()))?;
        let rows = if let Some(edge_type) = edge_type {
            stmt.query_map(params![to_symbol, edge_type, limit as isize], edge_from_row)
                .map_err(|e| GraphError::Database(e.to_string()))?
        } else {
            stmt.query_map(params![to_symbol, limit as isize], edge_from_row)
                .map_err(|e| GraphError::Database(e.to_string()))?
        };
        Ok(rows.filter_map(|row| row.ok()).collect())
    }

    pub fn symbol_by_stable_id(&self, stable_id: &str) -> Result<Option<Symbol>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let mut stmt = conn
            .prepare(
                r#"SELECT id, stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity
                   FROM symbols WHERE stable_id = ?1"#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;
        let mut rows = stmt
            .query_map(params![stable_id], |row| {
                Ok(Symbol {
                    id: Some(row.get(0)?),
                    stable_id: Some(row.get(1)?),
                    name: row.get(2)?,
                    kind: parse_symbol_kind(&row.get::<_, String>(3)?),
                    lang: parse_language(&row.get::<_, String>(4)?),
                    file_path: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    start_col: row.get(8)?,
                    end_col: row.get(9)?,
                    signature: row.get(10)?,
                    parent: row.get(11)?,
                    complexity: row.get(12)?,
                })
            })
            .map_err(|e| GraphError::Database(e.to_string()))?;
        Ok(rows.next().and_then(|row| row.ok()))
    }

    pub fn hub_nodes(&self, min_degree: u64, limit: usize) -> Result<Vec<HubNode>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT s.stable_id,
                       COALESCE(incoming.count, 0) AS incoming,
                       COALESCE(outgoing.count, 0) AS outgoing
                FROM symbols s
                LEFT JOIN (SELECT to_symbol AS stable_id, COUNT(*) AS count FROM edges GROUP BY to_symbol) incoming
                  ON incoming.stable_id = s.stable_id
                LEFT JOIN (SELECT from_symbol AS stable_id, COUNT(*) AS count FROM edges GROUP BY from_symbol) outgoing
                  ON outgoing.stable_id = s.stable_id
                WHERE COALESCE(incoming.count, 0) + COALESCE(outgoing.count, 0) >= ?1
                ORDER BY COALESCE(incoming.count, 0) + COALESCE(outgoing.count, 0) DESC
                LIMIT ?2
                "#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;
        let ids: Vec<(String, u64, u64)> = stmt
            .query_map(params![min_degree as i64, limit as isize], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                ))
            })
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|row| row.ok())
            .collect();
        drop(stmt);
        drop(conn);

        let mut hubs = Vec::new();
        for (stable_id, incoming, outgoing) in ids {
            if let Some(symbol) = self.symbol_by_stable_id(&stable_id)? {
                hubs.push(HubNode {
                    symbol,
                    incoming,
                    outgoing,
                    total: incoming + outgoing,
                });
            }
        }
        Ok(hubs)
    }

    pub fn complexity_hotspots(
        &self,
        min_complexity: f32,
        limit: usize,
    ) -> Result<Vec<ComplexityHotspot>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT s.stable_id,
                       COALESCE(incoming.count, 0) AS incoming,
                       COALESCE(outgoing.count, 0) AS outgoing,
                       COALESCE(s.complexity, 0.0) * (COALESCE(incoming.count, 0) + 1) AS risk
                FROM symbols s
                LEFT JOIN (SELECT to_symbol AS stable_id, COUNT(*) AS count FROM edges GROUP BY to_symbol) incoming
                  ON incoming.stable_id = s.stable_id
                LEFT JOIN (SELECT from_symbol AS stable_id, COUNT(*) AS count FROM edges GROUP BY from_symbol) outgoing
                  ON outgoing.stable_id = s.stable_id
                WHERE COALESCE(s.complexity, 0.0) >= ?1
                ORDER BY risk DESC, s.complexity DESC
                LIMIT ?2
                "#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;
        let rows: Vec<(String, u64, u64, f32)> = stmt
            .query_map(params![min_complexity, limit as isize], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, i64>(1)? as u64,
                    row.get::<_, i64>(2)? as u64,
                    row.get(3)?,
                ))
            })
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|row| row.ok())
            .collect();
        drop(stmt);
        drop(conn);

        let mut hotspots = Vec::new();
        for (stable_id, incoming, outgoing, risk_score) in rows {
            if let Some(symbol) = self.symbol_by_stable_id(&stable_id)? {
                hotspots.push(ComplexityHotspot {
                    symbol,
                    incoming,
                    outgoing,
                    risk_score,
                });
            }
        }
        Ok(hotspots)
    }

    /// Get all symbols in the database
    pub fn get_all_symbols(&self) -> Result<Vec<Symbol>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let mut stmt = conn
            .prepare(
                r#"SELECT id, stable_id, name, kind, lang, file_path, start_line, end_line, start_col, end_col, signature, parent, complexity
                   FROM symbols"#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let symbols = stmt
            .query_map([], |row| {
                Ok(Symbol {
                    id: Some(row.get(0)?),
                    stable_id: Some(row.get(1)?),
                    name: row.get(2)?,
                    kind: parse_symbol_kind(&row.get::<_, String>(3)?),
                    lang: parse_language(&row.get::<_, String>(4)?),
                    file_path: row.get(5)?,
                    start_line: row.get(6)?,
                    end_line: row.get(7)?,
                    start_col: row.get(8)?,
                    end_col: row.get(9)?,
                    signature: row.get(10)?,
                    parent: row.get(11)?,
                    complexity: row.get(12)?,
                })
            })
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(symbols)
    }

    /// Get all edges in the database
    pub fn get_all_edges(&self) -> Result<Vec<CodeEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let mut stmt = conn
            .prepare(
                r#"SELECT id, from_symbol, to_symbol, edge_type, file_path, line, confidence, metadata
                   FROM edges"#,
            )
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let edges = stmt
            .query_map([], edge_from_row)
            .map_err(|e| GraphError::Database(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(edges)
    }

    pub fn get_all_file_metadata(&self) -> Result<std::collections::HashMap<String, i64>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let mut stmt = conn
            .prepare("SELECT path, mtime FROM file_metadata")
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| GraphError::Database(e.to_string()))?;

        let mut metadata = std::collections::HashMap::new();
        for (path, mtime) in rows.flatten() {
            metadata.insert(path, mtime);
        }

        Ok(metadata)
    }

    /// Update or insert file metadata in batch
    pub fn batch_upsert_file_metadata(
        &self,
        metadata: std::collections::HashMap<String, i64>,
    ) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let tx = conn
            .transaction()
            .map_err(|e| GraphError::Database(e.to_string()))?;

        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO file_metadata (path, mtime) VALUES (?1, ?2)
                     ON CONFLICT(path) DO UPDATE SET mtime = excluded.mtime",
                )
                .map_err(|e| GraphError::Database(e.to_string()))?;

            for (path, mtime) in metadata {
                stmt.execute(params![path, mtime])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
            }
        }

        tx.commit()
            .map_err(|e| GraphError::Database(e.to_string()))?;

        Ok(())
    }

    /// Delete all data associated with multiple file paths in a single transaction
    pub fn batch_delete_file_data(&self, file_paths: &[String]) -> Result<()> {
        if file_paths.is_empty() {
            return Ok(());
        }

        let mut conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        let tx = conn
            .transaction()
            .map_err(|e| GraphError::Database(e.to_string()))?;

        {
            let mut stmt_symbols = tx
                .prepare("DELETE FROM symbols WHERE file_path = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;
            let mut stmt_fts = tx
                .prepare("DELETE FROM symbols_fts WHERE file_path = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;
            let mut stmt_edges = tx
                .prepare("DELETE FROM edges WHERE file_path = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;
            let mut stmt_refs = tx
                .prepare("DELETE FROM refs WHERE file_path = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;
            let mut stmt_imports = tx
                .prepare("DELETE FROM imports WHERE file_path = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;
            let mut stmt_meta = tx
                .prepare("DELETE FROM file_metadata WHERE path = ?1")
                .map_err(|e| GraphError::Database(e.to_string()))?;

            for path in file_paths {
                stmt_symbols
                    .execute(params![path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
                stmt_fts
                    .execute(params![path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
                stmt_edges
                    .execute(params![path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
                stmt_refs
                    .execute(params![path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
                stmt_imports
                    .execute(params![path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
                stmt_meta
                    .execute(params![path])
                    .map_err(|e| GraphError::Database(e.to_string()))?;
            }
        }

        tx.commit()
            .map_err(|e| GraphError::Database(e.to_string()))?;

        Ok(())
    }

    /// Clear all data
    pub fn clear(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| GraphError::Database(format!("lock poisoned: {}", e)))?;

        conn.execute_batch(
            r#"
            DELETE FROM refs;
            DELETE FROM imports;
            DELETE FROM edges;
            DELETE FROM symbols;
            DELETE FROM symbols_fts;
            DELETE FROM file_metadata;
            "#,
        )
        .map_err(|e| GraphError::Database(e.to_string()))?;

        info!("Database cleared");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text_language_values_from_sqlite() {
        assert_eq!(parse_language("Rust"), Language::Rust);
        assert_eq!(parse_language("TypeScript"), Language::TypeScript);
        assert_eq!(parse_language("unknown-value"), Language::Unknown);
    }

    #[test]
    fn parses_plain_text_symbol_kind_values_from_sqlite() {
        assert_eq!(parse_symbol_kind("Struct"), SymbolKind::Struct);
        assert_eq!(parse_symbol_kind("Function"), SymbolKind::Function);
        assert_eq!(parse_symbol_kind("unknown-value"), SymbolKind::Symbol);
    }

    #[test]
    fn deterministic_symbol_ids_upsert_on_reindex() {
        let db = CodeGraphDB::in_memory().expect("db");
        let symbol = Symbol {
            id: None,
            stable_id: None,
            name: "main".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/main.rs".to_string(),
            start_line: 1,
            end_line: 3,
            start_col: 0,
            end_col: 1,
            signature: Some("fn main()".to_string()),
            parent: None,
            complexity: Some(1.0),
        };

        db.insert_symbol(&symbol).expect("first insert");
        db.insert_symbol(&symbol).expect("second insert");

        let results = db.find_symbols("main", 10).expect("find");
        assert_eq!(results.symbols.len(), 1);
        assert!(results.symbols[0].stable_id.is_some());
    }

    #[test]
    fn handles_schema_migration_from_missing_stable_id() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("test.db");

        // Create a database with old schema (no stable_id)
        {
            let conn = Connection::open(&db_path).expect("failed to open db");
            conn.execute_batch(
                "CREATE TABLE symbols (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    lang TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL,
                    start_col INTEGER NOT NULL,
                    end_col INTEGER NOT NULL
                );",
            )
            .expect("failed to create old schema");
        }

        // Try to initialize CodeGraphDB, which should trigger schema migration
        let db = CodeGraphDB::new(&db_path);

        assert!(
            db.is_ok(),
            "Failed to initialize CodeGraphDB with old schema: {:?}",
            db.err()
        );

        let db = db.unwrap();
        // Check if stable_id was added
        let symbol = Symbol {
            id: None,
            stable_id: None,
            name: "test".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "test.rs".to_string(),
            start_line: 1,
            end_line: 1,
            start_col: 0,
            end_col: 0,
            signature: None,
            parent: None,
            complexity: None,
        };

        db.insert_symbol(&symbol).expect("failed to insert symbol");
        let results = db.find_symbols("test", 1).expect("failed to find symbols");
        assert_eq!(results.symbols.len(), 1);
        assert!(results.symbols[0].stable_id.is_some());
    }

    #[test]
    fn test_fts5_symbols_search_and_sync() {
        let db = CodeGraphDB::in_memory().expect("db");

        let sym1 = Symbol {
            id: None,
            stable_id: None,
            name: "process_data".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/processor.rs".to_string(),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 0,
            signature: Some("fn process_data()".to_string()),
            parent: None,
            complexity: None,
        };

        let sym2 = Symbol {
            id: None,
            stable_id: None,
            name: "calculate_total".to_string(),
            kind: SymbolKind::Function,
            lang: Language::Rust,
            file_path: "src/calc.rs".to_string(),
            start_line: 10,
            end_line: 15,
            start_col: 0,
            end_col: 0,
            signature: Some("fn calculate_total()".to_string()),
            parent: None,
            complexity: None,
        };

        db.insert_symbol(&sym1).expect("insert sym1");
        db.insert_symbol(&sym2).expect("insert sym2");

        // Exact match
        let res = db.find_symbols("process_data", 10).expect("find");
        assert_eq!(res.symbols.len(), 1);
        assert_eq!(res.symbols[0].name, "process_data");

        // Token match (FTS5 tokenizes on non-alphanumeric, so "calculate" matches "calculate_total")
        let res = db.find_symbols("calculate", 10).expect("find");
        assert_eq!(res.symbols.len(), 1);
        assert_eq!(res.symbols[0].name, "calculate_total");

        // Case-insensitive match
        let res = db.find_symbols("PROCESS", 10).expect("find");
        assert_eq!(res.symbols.len(), 1);
        assert_eq!(res.symbols[0].name, "process_data");

        // Empty query should fallback to LIKE and return everything
        let res = db.find_symbols("", 10).expect("find");
        assert!(res.symbols.len() >= 2);

        // Delete file sync verification
        db.batch_delete_file_data(&["src/processor.rs".to_string()])
            .expect("delete file");
        let res = db.find_symbols("process", 10).expect("find");
        assert_eq!(res.symbols.len(), 0);

        let res = db.find_symbols("calculate", 10).expect("find");
        assert_eq!(res.symbols.len(), 1);
    }
}
