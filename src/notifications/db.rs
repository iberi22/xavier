//! SQLite notifications table schema initialization and auto-migration.

#[cfg(test)]
use std::sync::RwLock;

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use tracing::info;

use xavier::codebase::connection_manager::ConnectionManager;
use xavier::espacio::keys::{looks_encrypted, KeyHandle, DEFAULT_SPACE_ID};
use xavier::memory::sqlite_vec_store::at_rest::{self, open_for_space, seal_for_space};
use xavier::notifications::{IslandId, Notification};

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

#[cfg(test)]
static INJECTED_KEY: RwLock<Option<KeyHandle>> = RwLock::new(None);

/// Set or clear process-wide injected key for tests.
#[cfg(test)]
pub fn set_injected_key_for_test(key: Option<KeyHandle>) {
    if let Ok(mut g) = INJECTED_KEY.write() {
        *g = key;
    }
}

/// Retrieve the process-wide injected key, if set.
#[cfg(test)]
pub fn get_injected_key() -> Option<KeyHandle> {
    INJECTED_KEY.read().ok().and_then(|g| g.clone())
}

/// Resolve the sealing key: injected key if set, then default-space key if keystore exists,
/// or None if no keystore exists. Fails closed if keystore exists but cannot be opened.
pub fn resolve_key(injected: Option<&KeyHandle>) -> Result<Option<KeyHandle>> {
    if let Some(k) = injected {
        return Ok(Some(k.clone()));
    }
    #[cfg(test)]
    if let Some(k) = get_injected_key() {
        return Ok(Some(k));
    }
    if !at_rest::default_keystore_exists() {
        return Ok(None);
    }
    at_rest::default_space_key()
        .map(Some)
        .context("default keystore exists but default-space key could not be opened")
}

/// Seal `title`/`body` for storage. Without a key the values stay in clear
/// (same degradation as the memory store).
pub fn seal_notification_text(
    dek: Option<&KeyHandle>,
    id: &str,
    title: &str,
    body: &str,
) -> Result<(String, String)> {
    match dek {
        Some(k) => seal_for_space(k, DEFAULT_SPACE_ID, id, title, body),
        None => Ok((title.to_string(), body.to_string())),
    }
}

/// Inverse of [`seal_notification_text`]. Legacy clear rows (no seal envelope
/// in `title`) are returned unchanged; a sealed row without a key is an error.
pub fn open_notification_text(
    dek: Option<&KeyHandle>,
    id: &str,
    title: &str,
    body: &str,
) -> Result<(String, String)> {
    if !looks_encrypted(title) {
        return Ok((title.to_string(), body.to_string()));
    }
    let k = dek.context("notification is sealed but no key is available")?;
    open_for_space(k, DEFAULT_SPACE_ID, id, title, body)
}

/// Insert a notification, sealing `title` and `body`.
#[allow(clippy::too_many_arguments)]
pub fn insert_notification_sealed(
    conn: &Connection,
    dek: Option<&KeyHandle>,
    id: &str,
    island_id: &str,
    title: &str,
    body: &str,
    timestamp: &str,
    read: i32,
    severity: &str,
) -> Result<()> {
    let (t, b) = seal_notification_text(dek, id, title, body)?;
    conn.execute(
        "INSERT INTO notifications (id, island_id, title, body, timestamp, read, severity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, island_id, t, b, timestamp, read, severity],
    )?;
    Ok(())
}

/// Read all notifications, opening sealed rows and passing legacy clear rows through.
pub fn load_notifications(conn: &Connection, dek: Option<&KeyHandle>) -> Result<Vec<Notification>> {
    let mut stmt = conn.prepare(
        "SELECT id, island_id, title, body, timestamp, read, severity FROM notifications ORDER BY timestamp DESC LIMIT 100",
    )?;
    let mut rows = stmt.query([])?;
    let mut result = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let island_str: String = row.get(1)?;
        let raw_title: String = row.get(2)?;
        let raw_body: String = row.get(3)?;
        let ts_str: String = row.get(4)?;
        let read: i32 = row.get(5)?;
        let severity: String = row.get(6)?;

        let (title, body) = open_notification_text(dek, &id, &raw_title, &raw_body)?;
        result.push(Notification {
            id,
            island_id: IslandId::from_str(&island_str),
            title,
            body,
            timestamp: ts_str.parse().unwrap_or_else(|_| chrono::Utc::now()),
            read: read != 0,
            severity,
        });
    }
    Ok(result)
}

/// Read `(id, title, body)` of every notification, opening sealed rows and
/// passing legacy clear rows through.
pub fn load_notification_texts(
    conn: &Connection,
    dek: Option<&KeyHandle>,
) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare("SELECT id, title, body FROM notifications ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, t, b) = row?;
        let (t, b) = open_notification_text(dek, &id, &t, &b)?;
        out.push((id, t, b));
    }
    Ok(out)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use xavier::espacio::keys::{KeyRing, StaticNodeKek, UnlockMode};

    const CANARY: &str = "NOTIF-CANARY-7f3a91c2";

    fn test_dek(dir: &std::path::Path) -> KeyHandle {
        let ring = KeyRing::new(dir, Arc::new(StaticNodeKek::new([0x42; 32])));
        ring.create_keys("default", UnlockMode::NodeUnlock, None)
            .unwrap()
            .0
    }

    #[test]
    fn notifications_adversary_no_plaintext_and_legacy_readable() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("notif.sqlite3");
        let dek = test_dek(&dir.path().join("keys"));
        let conn = Connection::open(&db_path).unwrap();
        init_notifications_schema_conn(&conn).unwrap();

        // Legacy row written in clear before sealing existed.
        conn.execute(
            "INSERT INTO notifications (id, island_id, title, body, timestamp, read, severity) VALUES ('legacy','system','legacy title','legacy body','2026-01-01T00:00:00Z',0,'info')",
            [],
        )
        .unwrap();

        insert_notification_sealed(
            &conn,
            Some(&dek),
            "new",
            "system",
            &format!("title {CANARY}"),
            &format!("body {CANARY}"),
            "2026-01-02T00:00:00Z",
            0,
            "info",
        )
        .unwrap();

        let rows = load_notification_texts(&conn, Some(&dek)).unwrap();
        assert!(rows.contains(&("legacy".into(), "legacy title".into(), "legacy body".into())));
        assert!(rows.contains(&(
            "new".into(),
            format!("title {CANARY}"),
            format!("body {CANARY}")
        )));
        // A sealed row cannot be opened without the key.
        assert!(load_notification_texts(&conn, None).is_err());
        drop(conn);

        let raw = std::fs::read(&db_path).unwrap();
        let wal = std::fs::read(dir.path().join("notif.sqlite3-wal")).unwrap_or_default();
        for bytes in [raw, wal] {
            assert!(!bytes.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()));
        }
    }
}
