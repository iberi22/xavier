//! SQLite notifications table schema initialization and auto-migration.

use anyhow::{Context, Result};
use rusqlite::Connection;
use tracing::info;

use xavier::codebase::connection_manager::ConnectionManager;

/// SQL DDL statement for creating the notifications table if it does not exist.
pub const CREATE_NOTIFICATIONS_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS notifications (
    id TEXT PRIMARY KEY,
    island_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    read INTEGER NOT NULL DEFAULT 0,
    severity TEXT NOT NULL
);
"#;

/// Expected notification table columns and their SQL column definitions for auto-migration.
pub const NOTIFICATION_COLUMNS: &[(&str, &str)] = &[
    ("id", "TEXT PRIMARY KEY"),
    ("island_id", "TEXT NOT NULL DEFAULT 'system'"),
    ("title", "TEXT NOT NULL DEFAULT ''"),
    ("body", "TEXT NOT NULL DEFAULT ''"),
    ("timestamp", "TEXT NOT NULL DEFAULT ''"),
    ("read", "INTEGER NOT NULL DEFAULT 0"),
    ("severity", "TEXT NOT NULL DEFAULT 'info'"),
];

/// Synchronously initializes the notifications table schema and auto-migrates missing columns on the given SQLite connection.
pub fn init_notifications_schema_conn(conn: &Connection) -> Result<()> {
    crate::storage::apply_pragmas(conn)
        .context("Failed to apply SQLite pragmas for notifications database")?;

    conn.execute_batch(CREATE_NOTIFICATIONS_TABLE_DDL)
        .context("Failed to execute CREATE TABLE IF NOT EXISTS notifications DDL")?;

    let existing_cols: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(notifications)")
            .context("Failed to prepare PRAGMA table_info(notifications)")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .context("Failed to query PRAGMA table_info(notifications)")?;
        let mut cols = Vec::new();
        for col in rows {
            cols.push(col.context("Error reading column name from PRAGMA table_info")?);
        }
        cols
    };

    for &(col_name, col_def) in NOTIFICATION_COLUMNS {
        if !existing_cols.contains(&col_name.to_string()) {
            info!(
                "Auto-migrating notifications table: adding missing column '{}'",
                col_name
            );
            let alter_sql = format!(
                "ALTER TABLE notifications ADD COLUMN {} {}",
                col_name, col_def
            );
            conn.execute(&alter_sql, [])
                .with_context(|| format!("Failed to auto-migrate column '{}'", col_name))?;
        }
    }

    Ok(())
}

/// Initializes the notifications table schema asynchronously using ConnectionManager.
pub async fn init_notifications_schema() -> Result<()> {
    let cm = ConnectionManager::global();
    if cm.with_conn("memory", |_| Ok(())).await.is_err() {
        let root = std::env::var("XAVIER_DATA_DIR").unwrap_or_else(|_| ".".to_string());
        cm.connect("memory", &root)?;
    }

    cm.with_conn("memory", init_notifications_schema_conn).await
}
