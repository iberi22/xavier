//! # Observability Integration Tests
//!
//! End-to-end tests for the observability module.
//! Since ServiceLogStore depends on ConnectionManager::global() which requires
//! a real project root, we test the core logic using raw SQLite connections
//! and the public types from the observability module.

#![cfg(test)]

use rusqlite::Connection;
use serde_json::json;
use xavier::observability::service_log::*;
use xavier::observability::*;

// â”€â”€ Helper: create an in-memory SQLite DB with the service_logs schema â”€â”€

fn create_memory_db() -> anyhow::Result<Connection> {
    let conn = Connection::open_in_memory()?;
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

        CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
            message, metadata,
            content='service_logs', content_rowid='rowid'
        );
    "#;
    conn.execute_batch(sql)?;
    Ok(conn)
}

/// Insert a LogEntry directly into a raw Connection (bypassing ServiceLogStore).
fn insert_log(conn: &Connection, entry: &LogEntry) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO service_logs (id, timestamp, level, source, module, correlation_id, message, metadata, resolved, resolution)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            entry.id,
            entry.timestamp,
            entry.level.to_string(),
            entry.source.to_string(),
            entry.module,
            entry.correlation_id,
            entry.message,
            entry.metadata.as_ref().map(|m| m.to_string()),
            entry.resolved as i32,
            entry.resolution.as_ref().map(|r| r.to_string()),
        ],
    )?;
    Ok(())
}

/// Count total rows in service_logs.
fn count_logs(conn: &Connection) -> anyhow::Result<u64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM service_logs", [], |r| r.get(0))?)
}

/// Get observability stats via direct SQL (mirrors get_stats logic).
fn get_stats_direct(conn: &Connection) -> anyhow::Result<ObservabilityStats> {
    let total_entries: u64 = conn
        .query_row("SELECT COUNT(*) FROM service_logs", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(ObservabilityStats {
        total_entries,
        errors_last_hour: 0,
        errors_today: 0,
        warnings_today: 0,
        active_patterns: 0,
        uptime_seconds: 0,
        db_size_kb: 0,
    })
}

// â”€â”€ Integration Tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_full_pipeline_end_to_end() -> anyhow::Result<()> {
    let conn = create_memory_db()?;

    // Step 1: Log an error
    let entry = LogEntry::error(
        LogSource::HttpServer,
        "http::api",
        "500 Internal Server Error",
    )
    .with_metadata(json!({"method": "POST", "path": "/api/data"}));
    insert_log(&conn, &entry)?;
    assert_eq!(count_logs(&conn)?, 1);

    // Step 2: Query stats
    let stats = get_stats_direct(&conn)?;
    assert_eq!(stats.total_entries, 1);

    // Step 3: Detect patterns (via raw SQL matching ServiceLogStore::detect_patterns)
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM service_logs WHERE level IN ('error', 'warn')",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 1);

    // Step 4: FTS5 search
    conn.execute(
        "INSERT INTO logs_fts(rowid, message, metadata) SELECT rowid, message, metadata FROM service_logs",
        [],
    )?;
    let results: u32 = conn.query_row(
        "SELECT COUNT(*) FROM logs_fts WHERE logs_fts MATCH '500'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(results, 1);

    // Step 5: Resolve
    conn.execute(
        "UPDATE service_logs SET resolved = 1, resolution = ?1 WHERE id = ?2",
        rusqlite::params![json!({"fix": "restarted"}).to_string(), entry.id],
    )?;
    let resolved: bool = conn.query_row(
        "SELECT resolved FROM service_logs WHERE id = ?1",
        rusqlite::params![entry.id],
        |r| r.get::<_, i32>(0).map(|v| v != 0),
    )?;
    assert!(resolved);

    Ok(())
}

#[tokio::test]
async fn test_concurrent_log_writes() -> anyhow::Result<()> {
    let conn = std::sync::Arc::new(std::sync::Mutex::new(create_memory_db()?));
    let num_logs = 50;

    let mut handles = Vec::new();
    for i in 0..num_logs {
        let conn = conn.clone();
        handles.push(tokio::spawn(async move {
            let entry = LogEntry::error(
                LogSource::AgentRuntime,
                "concurrent",
                &format!("error #{}", i),
            );
            let conn = conn.lock().unwrap();
            insert_log(&conn, &entry).unwrap();
        }));
    }

    for h in handles {
        h.await?;
    }

    let conn = conn.lock().unwrap();
    assert_eq!(count_logs(&conn)?, num_logs as u64);
    Ok(())
}

#[tokio::test]
async fn test_error_with_correlation_id() -> anyhow::Result<()> {
    let conn = create_memory_db()?;

    // Log multiple events with the same correlation ID
    let cid = "corr-chain-001";
    let entries = vec![
        LogEntry::error(LogSource::HttpServer, "api::auth", "token expired")
            .with_correlation_id(cid),
        LogEntry::warn(LogSource::HttpServer, "api::auth", "retrying").with_correlation_id(cid),
        LogEntry::info(LogSource::HttpServer, "api::auth", "recovered").with_correlation_id(cid),
    ];

    for entry in &entries {
        insert_log(&conn, entry)?;
    }

    assert_eq!(count_logs(&conn)?, 3);

    // Verify correlation group
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM service_logs WHERE correlation_id = ?1",
        rusqlite::params![cid],
        |r| r.get(0),
    )?;
    assert_eq!(count, 3);

    Ok(())
}

#[tokio::test]
async fn test_log_levels_and_sources() -> anyhow::Result<()> {
    let conn = create_memory_db()?;

    let entries = vec![
        LogEntry::error(LogSource::HttpServer, "http", "error"),
        LogEntry::warn(LogSource::Sidecar, "sidecar", "warn"),
        LogEntry::info(LogSource::AgentRuntime, "agent", "info"),
        LogEntry::debug(LogSource::Cli, "cli", "debug"),
    ];

    for e in &entries {
        insert_log(&conn, e)?;
    }

    assert_eq!(count_logs(&conn)?, 4);

    // Verify different sources
    for source in &["http_server", "sidecar", "agent_runtime", "cli"] {
        let count: u32 = conn.query_row(
            "SELECT COUNT(*) FROM service_logs WHERE source = ?1",
            rusqlite::params![source],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
    }

    Ok(())
}

#[tokio::test]
async fn test_pipeline_log_detect_analyze() -> anyhow::Result<()> {
    // Simulate: log error -> detect pattern -> generate diagnosis
    let conn = create_memory_db()?;

    // Log several errors with the same module and message
    for _ in 0..5 {
        let entry = LogEntry::error(
            LogSource::HttpServer,
            "api::handler",
            "database timeout: connection pool exhausted",
        );
        insert_log(&conn, &entry)?;
    }

    // Detect: group by module + message prefix, count >= 3
    let patterns: Vec<(String, u32)> = {
        let mut stmt = conn.prepare(
            r#"
            SELECT module, COUNT(*) as freq
            FROM service_logs
            WHERE level IN ('error', 'warn')
            GROUP BY module, SUBSTRING(message, 1, 200)
            HAVING freq >= 3
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].0, "api::handler");
    assert!(patterns[0].1 >= 5);

    // Analyze: classify the error
    let analyzer = ErrorAnalyzer::new();
    let pattern = ErrorPattern {
        module: "api::handler".into(),
        level: LogLevel::Error,
        frequency: 5,
        sample_message: "database timeout: connection pool exhausted".into(),
        first_seen: "t1".into(),
        last_seen: "t2".into(),
    };
    let diagnosis = analyzer.analyze(&pattern).await;
    assert!(diagnosis.root_cause.contains("Database"));
    assert_eq!(diagnosis.urgency, analyzer::Urgency::Critical);

    Ok(())
}

#[tokio::test]
async fn test_fts5_search_with_special_chars() -> anyhow::Result<()> {
    let conn = create_memory_db()?;

    let entry = LogEntry::error(
        LogSource::HttpServer,
        "api",
        "connection refused: tcp://localhost:3000",
    );
    insert_log(&conn, &entry)?;

    conn.execute(
        "INSERT INTO logs_fts(rowid, message, metadata) SELECT rowid, message, metadata FROM service_logs",
        [],
    )?;

    // FTS5 handles special chars via quoting
    let result: u32 = conn.query_row(
        "SELECT COUNT(*) FROM logs_fts WHERE logs_fts MATCH 'refused'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(result, 1);

    Ok(())
}

#[tokio::test]
async fn test_resolve_multiple_entries() -> anyhow::Result<()> {
    let conn = create_memory_db()?;

    let ids: Vec<String> = (0..3)
        .map(|i| {
            let entry = LogEntry::error(LogSource::HttpServer, "mod", &format!("error {}", i));
            let id = entry.id.clone();
            insert_log(&conn, &entry).unwrap();
            id
        })
        .collect();

    // Resolve all
    for id in &ids {
        conn.execute(
            "UPDATE service_logs SET resolved = 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
    }

    let unresolved: u32 = conn.query_row(
        "SELECT COUNT(*) FROM service_logs WHERE resolved = 0",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(unresolved, 0);

    Ok(())
}
