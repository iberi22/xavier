//! # Service Log Store
//!
//! Persistente log storage in Xavier's `vec-store.sqlite3` database.
//! Supports structured logging, FTS5 full-text search, and pattern queries.
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS service_logs (
//!     id              TEXT PRIMARY KEY,
//!     timestamp       TEXT NOT NULL,      -- ISO 8601
//!     level           TEXT NOT NULL,      -- error | warn | info | debug | trace
//!     source          TEXT NOT NULL,      -- http_server | agent_runtime | sidecar | ui | cli
//!     module          TEXT,
//!     correlation_id  TEXT,               -- request chain correlation
//!     message         TEXT NOT NULL,
//!     metadata        TEXT,               -- JSON blob (stack_trace, body, user, etc.)
//!     resolved        INTEGER DEFAULT 0,
//!     resolution      TEXT                -- JSON (fix_applied, pr_number, notes)
//! );
//! ```
//!
//! FTS5 virtual table for full-text search:
//! ```sql
//! CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
//!     message, metadata,
//!     content='service_logs', content_rowid='rowid'
//! );
//! ```

use crate::codebase::connection_manager::ConnectionManager;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Log severity levels, sorted by priority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "trace"),
            LogLevel::Debug => write!(f, "debug"),
            LogLevel::Info => write!(f, "info"),
            LogLevel::Warn => write!(f, "warn"),
            LogLevel::Error => write!(f, "error"),
        }
    }
}

impl From<&str> for LogLevel {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

/// Source system that generated the log entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    HttpServer,
    AgentRuntime,
    Sidecar,
    Ui,
    Cli,
    Scheduler,
    Detector,
    Analyzer,
    Fixer,
    Notifier,
    Other(String),
}

impl std::fmt::Display for LogSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogSource::HttpServer => write!(f, "http_server"),
            LogSource::AgentRuntime => write!(f, "agent_runtime"),
            LogSource::Sidecar => write!(f, "sidecar"),
            LogSource::Ui => write!(f, "ui"),
            LogSource::Cli => write!(f, "cli"),
            LogSource::Scheduler => write!(f, "scheduler"),
            LogSource::Detector => write!(f, "detector"),
            LogSource::Analyzer => write!(f, "analyzer"),
            LogSource::Fixer => write!(f, "fixer"),
            LogSource::Notifier => write!(f, "notifier"),
            LogSource::Other(s) => write!(f, "{}", s),
        }
    }
}

impl From<&str> for LogSource {
    fn from(s: &str) -> Self {
        match s {
            "http_server" => LogSource::HttpServer,
            "agent_runtime" => LogSource::AgentRuntime,
            "sidecar" => LogSource::Sidecar,
            "ui" => LogSource::Ui,
            "cli" => LogSource::Cli,
            "scheduler" => LogSource::Scheduler,
            "detector" => LogSource::Detector,
            "analyzer" => LogSource::Analyzer,
            "fixer" => LogSource::Fixer,
            "notifier" => LogSource::Notifier,
            other => LogSource::Other(other.to_string()),
        }
    }
}

/// A single log entry stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub level: LogLevel,
    pub source: LogSource,
    pub module: Option<String>,
    pub correlation_id: Option<String>,
    pub message: String,
    pub metadata: Option<serde_json::Value>,
    pub resolved: bool,
    pub resolution: Option<serde_json::Value>,
}

impl LogEntry {
    /// Create a new error log entry.
    pub fn error(source: LogSource, module: &str, message: &str) -> Self {
        Self::new(LogLevel::Error, source, module, message)
    }

    /// Create a new warning log entry.
    pub fn warn(source: LogSource, module: &str, message: &str) -> Self {
        Self::new(LogLevel::Warn, source, module, message)
    }

    /// Create a new info log entry.
    pub fn info(source: LogSource, module: &str, message: &str) -> Self {
        Self::new(LogLevel::Info, source, module, message)
    }

    /// Create a new debug log entry.
    pub fn debug(source: LogSource, module: &str, message: &str) -> Self {
        Self::new(LogLevel::Debug, source, module, message)
    }

    fn new(level: LogLevel, source: LogSource, module: &str, message: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            level,
            source,
            module: Some(module.to_string()),
            correlation_id: None,
            message: message.to_string(),
            metadata: None,
            resolved: false,
            resolution: None,
        }
    }

    /// Add correlation ID (links related requests).
    pub fn with_correlation_id(mut self, cid: &str) -> Self {
        self.correlation_id = Some(cid.to_string());
        self
    }

    /// Add metadata as JSON value.
    pub fn with_metadata(mut self, meta: serde_json::Value) -> Self {
        self.metadata = Some(meta);
        self
    }

    /// Mark as resolved with resolution info.
    pub fn with_resolution(mut self, resolution: serde_json::Value) -> Self {
        self.resolved = true;
        self.resolution = Some(resolution);
        self
    }
}

/// Detected error pattern (grouped by module + message fingerprint).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorPattern {
    pub module: String,
    pub level: LogLevel,
    pub frequency: u32,
    pub sample_message: String,
    pub first_seen: String,
    pub last_seen: String,
}

/// Statistics for the observability dashboard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityStats {
    pub total_entries: u64,
    pub errors_last_hour: u64,
    pub errors_today: u64,
    pub warnings_today: u64,
    pub active_patterns: u64,
    pub uptime_seconds: u64,
    pub db_size_kb: u64,
}

/// The service log store â€” writes/reads to vec-store.sqlite3.
#[derive(Clone)]
pub struct ServiceLogStore {
    conn: &'static ConnectionManager,
}

impl ServiceLogStore {
    /// Create a new ServiceLogStore, ensuring the table exists.
    pub async fn new() -> Result<Self> {
        let conn = ConnectionManager::global();
        let store = Self { conn };
        store.initialize_schema().await?;
        Ok(store)
    }

    /// Create the service_logs table + FTS5 index if not exists.
    async fn initialize_schema(&self) -> Result<()> {
        let sql = r#"
            CREATE TABLE IF NOT EXISTS service_logs (
                id              TEXT PRIMARY KEY,
                timestamp       TEXT NOT NULL,
                level           TEXT NOT NULL,
                source          TEXT NOT NULL,
                module          TEXT,
                correlation_id  TEXT,
                message         TEXT NOT NULL,
                metadata        TEXT,
                resolved        INTEGER DEFAULT 0,
                resolution      TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON service_logs(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_logs_level ON service_logs(level);
            CREATE INDEX IF NOT EXISTS idx_logs_module ON service_logs(module);
            CREATE INDEX IF NOT EXISTS idx_logs_source ON service_logs(source);
            CREATE INDEX IF NOT EXISTS idx_logs_resolved ON service_logs(resolved);
            CREATE INDEX IF NOT EXISTS idx_logs_level_module ON service_logs(level, module);

            CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
                message, metadata,
                content='service_logs', content_rowid='rowid'
            );
        "#;

        self.conn
            .with_conn("vec_store", |conn| {
                conn.execute_batch(sql)
                    .context("Failed to create service_logs schema")
            })
            .await?;

        tracing::debug!("Service logs schema initialized");
        Ok(())
    }

    /// Insert a log entry into the database.
    pub async fn log(&self, entry: LogEntry) -> Result<String> {
        let id = entry.id.clone();
        let params = (
            entry.id.clone(),
            entry.timestamp.clone(),
            entry.level.to_string(),
            entry.source.to_string(),
            entry.module.clone(),
            entry.correlation_id.clone(),
            entry.message.clone(),
            entry.metadata.clone().map(|m| m.to_string()),
            entry.resolved as i32,
            entry.resolution.clone().map(|r| r.to_string()),
        );

        self.conn.with_conn("vec_store", move |conn| {
            conn.execute(
                "INSERT INTO service_logs (id, timestamp, level, source, module, correlation_id, message, metadata, resolved, resolution)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params.clone(),
            )?;
            Ok(id.clone())
        }).await
    }

    /// Query recent errors for a specific module.
    pub async fn query_recent_errors(
        &self,
        module: &str,
        limit: u32,
        minutes: u32,
    ) -> Result<Vec<LogEntry>> {
        let module = module.to_string();
        self.conn.with_conn("vec_store", move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, timestamp, level, source, module, correlation_id, message, metadata, resolved, resolution
                 FROM service_logs
                 WHERE level = 'error'
                   AND module = ?1
                   AND timestamp > datetime('now', printf('-%d minutes', ?3))
                 ORDER BY timestamp DESC
                 LIMIT ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![module, limit, minutes], Self::map_row)?;
            let mut entries = Vec::new();
            for row in rows {
                entries.push(row?);
            }
            Ok(entries)
        }).await
    }

    /// Query recent log entries, optionally filtered by level and source.
    ///
    /// Returns the most recent entries first. `level` and `source` are optional
    /// lowercase filters (e.g. `"error"`, `"http_server"`); pass `None` to skip.
    pub async fn query_recent(
        &self,
        level: Option<&str>,
        source: Option<&str>,
        limit: u32,
    ) -> Result<Vec<LogEntry>> {
        // Build the query dynamically. `level`/`source` are constrained to known
        // enum variants upstream, so interpolating the literal into the WHERE
        // clause is safe here (no user-supplied free text).
        let mut where_clauses = Vec::new();
        if level.is_some() {
            where_clauses.push("level = :L");
        }
        if source.is_some() {
            where_clauses.push("source = :S");
        }
        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let level = level.map(|s| s.to_string());
        let source = source.map(|s| s.to_string());

        self.conn
            .with_conn("vec_store", move |conn| {
                let sql = format!(
                    "SELECT id, timestamp, level, source, module, correlation_id, \
                     message, metadata, resolved, resolution \
                     FROM service_logs {where_sql} \
                     ORDER BY timestamp DESC LIMIT :N"
                );
                let mut stmt = conn.prepare(&sql)?;

                // Bind dynamically depending on which filters are present.
                let rows = if let (Some(l), Some(s)) = (&level, &source) {
                    stmt.query_map(
                        rusqlite::named_params! { ":L": l, ":S": s, ":N": limit },
                        Self::map_row,
                    )?
                } else if let Some(l) = &level {
                    stmt.query_map(
                        rusqlite::named_params! { ":L": l, ":N": limit },
                        Self::map_row,
                    )?
                } else if let Some(s) = &source {
                    stmt.query_map(
                        rusqlite::named_params! { ":S": s, ":N": limit },
                        Self::map_row,
                    )?
                } else {
                    stmt.query_map(rusqlite::named_params! { ":N": limit }, Self::map_row)?
                };

                let mut entries = Vec::new();
                for row in rows {
                    entries.push(row?);
                }
                Ok(entries)
            })
            .await
    }

    /// Detect error patterns: same module + message repeated > threshold times.
    pub async fn detect_patterns(&self, minutes: u32, threshold: u32) -> Result<Vec<ErrorPattern>> {
        self.conn
            .with_conn("vec_store", move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                SELECT
                    module,
                    level,
                    COUNT(*) as freq,
                    SUBSTRING(message, 1, 200) as sample,
                    MIN(timestamp) as first_seen,
                    MAX(timestamp) as last_seen
                FROM service_logs
                WHERE timestamp > datetime('now', printf('-%d minutes', ?1))
                  AND level IN ('error', 'warn')
                GROUP BY module, SUBSTRING(message, 1, 200)
                HAVING freq >= ?2
                ORDER BY freq DESC
                "#,
                )?;

                let rows = stmt.query_map(rusqlite::params![minutes, threshold], |row| {
                    Ok(ErrorPattern {
                        module: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                        level: {
                            let s: String =
                                row.get::<_, String>(1).unwrap_or_else(|_| "info".into());
                            LogLevel::from(s.as_str())
                        },
                        frequency: row.get::<_, u32>(2)?,
                        sample_message: row.get(3)?,
                        first_seen: row.get(4)?,
                        last_seen: row.get(5)?,
                    })
                })?;

                let mut patterns = Vec::new();
                for row in rows {
                    patterns.push(row?);
                }
                Ok(patterns)
            })
            .await
    }

    /// Full-text search across log messages and metadata.
    pub async fn search_logs(&self, query: &str, limit: u32) -> Result<Vec<LogEntry>> {
        let query = query.to_string();
        self.conn
            .with_conn("vec_store", move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                SELECT sl.id, sl.timestamp, sl.level, sl.source, sl.module,
                       sl.correlation_id, sl.message, sl.metadata, sl.resolved, sl.resolution
                FROM logs_fts
                JOIN service_logs sl ON logs_fts.rowid = sl.rowid
                WHERE logs_fts MATCH ?1
                ORDER BY rank
                LIMIT ?2
                "#,
                )?;

                let rows = stmt.query_map(rusqlite::params![query, limit], Self::map_row)?;
                let mut entries = Vec::new();
                for row in rows {
                    entries.push(row?);
                }
                Ok(entries)
            })
            .await
    }

    /// Get aggregate statistics for the monitor dashboard.
    pub async fn get_stats(&self) -> Result<ObservabilityStats> {
        self.conn.with_conn("vec_store", |conn| {
            let total_entries: u64 = conn
                .query_row("SELECT COUNT(*) FROM service_logs", [], |r| r.get(0))
                .unwrap_or(0);

            let errors_last_hour: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM service_logs WHERE level = 'error' AND timestamp > datetime('now', '-1 hour')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let errors_today: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM service_logs WHERE level = 'error' AND timestamp > datetime('now', '-24 hours')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let warnings_today: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM service_logs WHERE level = 'warn' AND timestamp > datetime('now', '-24 hours')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let active_patterns: u64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT module || SUBSTRING(message, 1, 100))
                     FROM service_logs WHERE level = 'error' AND resolved = 0",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            Ok(ObservabilityStats {
                total_entries,
                errors_last_hour,
                errors_today,
                warnings_today,
                active_patterns,
                uptime_seconds: 0,
                db_size_kb: 0,
            })
        }).await
    }

    /// Update the resolution for a log entry (mark as fixed).
    pub async fn resolve(&self, id: &str, resolution: serde_json::Value) -> Result<()> {
        let id = id.to_string();
        let resolution_str = resolution.to_string();
        self.conn
            .with_conn("vec_store", move |conn| {
                conn.execute(
                    "UPDATE service_logs SET resolved = 1, resolution = ?1 WHERE id = ?2",
                    rusqlite::params![resolution_str, id],
                )?;
                Ok(())
            })
            .await
    }

    // â”€â”€ Helper: map SQLite row to LogEntry â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn map_row(row: &rusqlite::Row) -> rusqlite::Result<LogEntry> {
        let metadata_str: Option<String> = row.get(7)?;
        let resolution_str: Option<String> = row.get(9)?;

        Ok(LogEntry {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            level: {
                let s: String = row.get(2)?;
                LogLevel::from(s.as_str())
            },
            source: {
                let s: String = row.get(3)?;
                LogSource::from(s.as_str())
            },
            module: row.get(4)?,
            correlation_id: row.get(5)?,
            message: row.get(6)?,
            metadata: metadata_str.and_then(|s| serde_json::from_str(&s).ok()),
            resolved: row.get::<_, i32>(8)? != 0,
            resolution: resolution_str.and_then(|s| serde_json::from_str(&s).ok()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_log_entry_creation() {
        let entry = LogEntry::error(LogSource::HttpServer, "test_mod", "something broke");
        assert!(entry.id.len() > 10);
        assert!(!entry.timestamp.is_empty());
        assert_eq!(entry.level, LogLevel::Error);
        assert!(entry.module.as_deref() == Some("test_mod"));
        assert_eq!(entry.message, "something broke");
        assert!(!entry.resolved);
        assert!(entry.resolution.is_none());
    }

    #[tokio::test]
    async fn test_log_entry_info() {
        let entry = LogEntry::info(LogSource::AgentRuntime, "agent::chat", "started");
        assert_eq!(entry.level, LogLevel::Info);
    }

    #[tokio::test]
    async fn test_log_entry_warn() {
        let entry = LogEntry::warn(LogSource::Sidecar, "sidecar", "high memory");
        assert_eq!(entry.level, LogLevel::Warn);
    }

    #[tokio::test]
    async fn test_log_entry_debug() {
        let entry = LogEntry::debug(LogSource::Cli, "cli", "parsing args");
        assert_eq!(entry.level, LogLevel::Debug);
    }

    #[tokio::test]
    async fn test_log_entry_with_correlation_id() {
        let entry =
            LogEntry::error(LogSource::HttpServer, "http", "err").with_correlation_id("req-123");
        assert_eq!(entry.correlation_id.as_deref(), Some("req-123"));
    }

    #[tokio::test]
    async fn test_log_entry_with_metadata() {
        let meta = serde_json::json!({"user": "bob", "status": 500});
        let entry =
            LogEntry::error(LogSource::HttpServer, "api", "fail").with_metadata(meta.clone());
        assert_eq!(entry.metadata, Some(meta));
    }

    #[tokio::test]
    async fn test_log_entry_with_resolution() {
        let res = serde_json::json!({"fix": "restarted"});
        let entry =
            LogEntry::error(LogSource::HttpServer, "mod", "crash").with_resolution(res.clone());
        assert!(entry.resolved);
        assert_eq!(entry.resolution, Some(res));
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Trace.to_string(), "trace");
        assert_eq!(LogLevel::Debug.to_string(), "debug");
        assert_eq!(LogLevel::Info.to_string(), "info");
        assert_eq!(LogLevel::Warn.to_string(), "warn");
        assert_eq!(LogLevel::Error.to_string(), "error");
    }

    #[test]
    fn test_log_level_from_str() {
        assert_eq!(LogLevel::from("error"), LogLevel::Error);
        assert_eq!(LogLevel::from("warn"), LogLevel::Warn);
        assert_eq!(LogLevel::from("warning"), LogLevel::Warn);
        assert_eq!(LogLevel::from("info"), LogLevel::Info);
        assert_eq!(LogLevel::from("debug"), LogLevel::Debug);
        assert_eq!(LogLevel::from("trace"), LogLevel::Trace);
        assert_eq!(LogLevel::from("unknown"), LogLevel::Info);
        assert_eq!(LogLevel::from("ERROR"), LogLevel::Error);
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Error > LogLevel::Warn);
        assert!(LogLevel::Warn > LogLevel::Info);
        assert!(LogLevel::Info > LogLevel::Debug);
        assert!(LogLevel::Debug > LogLevel::Trace);
    }

    #[test]
    fn test_log_source_display() {
        assert_eq!(LogSource::HttpServer.to_string(), "http_server");
        assert_eq!(LogSource::AgentRuntime.to_string(), "agent_runtime");
        assert_eq!(LogSource::Sidecar.to_string(), "sidecar");
        assert_eq!(LogSource::Ui.to_string(), "ui");
        assert_eq!(LogSource::Cli.to_string(), "cli");
        assert_eq!(LogSource::Scheduler.to_string(), "scheduler");
        assert_eq!(LogSource::Detector.to_string(), "detector");
        assert_eq!(LogSource::Analyzer.to_string(), "analyzer");
        assert_eq!(LogSource::Fixer.to_string(), "fixer");
        assert_eq!(LogSource::Notifier.to_string(), "notifier");
        assert_eq!(LogSource::Other("custom".into()).to_string(), "custom");
    }

    #[test]
    fn test_log_source_from_str() {
        assert_eq!(LogSource::from("http_server"), LogSource::HttpServer);
        assert_eq!(LogSource::from("agent_runtime"), LogSource::AgentRuntime);
        assert_eq!(LogSource::from("sidecar"), LogSource::Sidecar);
        assert_eq!(LogSource::from("ui"), LogSource::Ui);
        assert_eq!(LogSource::from("cli"), LogSource::Cli);
        assert_eq!(LogSource::from("scheduler"), LogSource::Scheduler);
        assert_eq!(LogSource::from("detector"), LogSource::Detector);
        assert_eq!(LogSource::from("analyzer"), LogSource::Analyzer);
        assert_eq!(LogSource::from("fixer"), LogSource::Fixer);
        assert_eq!(LogSource::from("notifier"), LogSource::Notifier);
        assert_eq!(
            LogSource::from("unknown"),
            LogSource::Other("unknown".into())
        );
    }

    #[test]
    fn test_error_pattern_struct() {
        let p = ErrorPattern {
            module: "http".into(),
            level: LogLevel::Error,
            frequency: 5,
            sample_message: "timeout".into(),
            first_seen: "t1".into(),
            last_seen: "t2".into(),
        };
        assert_eq!(p.module, "http");
        assert_eq!(p.frequency, 5);
    }
}
