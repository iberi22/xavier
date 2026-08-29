//! Integration tests for notifications database schema auto-creation and migration.

#[path = "../src/notifications/db.rs"]
mod db;

use rusqlite::Connection;

#[test]
fn test_init_notifications_schema_fresh_db() {
    let conn = Connection::open_in_memory().expect("failed to open in-memory db");

    db::init_notifications_schema_conn(&conn).expect("schema initialization failed");

    // Verify all columns exist via PRAGMA table_info
    let mut stmt = conn
        .prepare("PRAGMA table_info(notifications)")
        .expect("failed to prepare pragma statement");
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("failed to query pragma table_info")
        .map(|r| r.unwrap())
        .collect();

    let expected_columns = vec![
        "id",
        "island_id",
        "title",
        "body",
        "timestamp",
        "read",
        "severity",
    ];

    for col in expected_columns {
        assert!(
            columns.contains(&col.to_string()),
            "missing column '{}' in notifications table",
            col
        );
    }

    // Verify inserting and retrieving a notification record
    conn.execute(
        "INSERT INTO notifications (id, island_id, title, body, timestamp, read, severity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params!["test-1", "system", "Test Title", "Test Body", "2026-01-01T00:00:00Z", 0, "info"],
    )
    .expect("failed to insert notification");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM notifications WHERE id = 'test-1'",
            [],
            |r| r.get(0),
        )
        .expect("query failed");
    assert_eq!(count, 1);
}

#[test]
fn test_init_notifications_schema_idempotency() {
    let conn = Connection::open_in_memory().expect("failed to open in-memory db");

    // Executing schema init multiple times must be idempotent
    db::init_notifications_schema_conn(&conn).expect("first schema init failed");
    db::init_notifications_schema_conn(&conn).expect("second schema init failed");
    db::init_notifications_schema_conn(&conn).expect("third schema init failed");

    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='notifications'",
            [],
            |r| r.get(0),
        )
        .expect("table check failed");
    assert_eq!(count, 1);
}

#[test]
fn test_init_notifications_schema_auto_migration() {
    let conn = Connection::open_in_memory().expect("failed to open in-memory db");

    // Simulate an incomplete legacy table missing island_id, timestamp, read, severity
    conn.execute_batch(
        "CREATE TABLE notifications (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            body TEXT NOT NULL
        );",
    )
    .expect("creation of legacy partial table failed");

    // Auto-migration should add the missing columns
    db::init_notifications_schema_conn(&conn).expect("auto-migration failed");

    let mut stmt = conn
        .prepare("PRAGMA table_info(notifications)")
        .expect("pragma statement failed");
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .expect("pragma query failed")
        .map(|r| r.unwrap())
        .collect();

    assert!(columns.contains(&"island_id".to_string()));
    assert!(columns.contains(&"timestamp".to_string()));
    assert!(columns.contains(&"read".to_string()));
    assert!(columns.contains(&"severity".to_string()));

    // Verify insertion works with all columns
    conn.execute(
        "INSERT INTO notifications (id, island_id, title, body, timestamp, read, severity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params!["test-migrated", "errors", "Alert", "Disk Space", "2026-01-01T00:00:00Z", 0, "error"],
    )
    .expect("failed to insert after auto-migration");
}

#[tokio::test]
async fn test_init_notifications_schema_async() {
    let result = db::init_notifications_schema().await;
    assert!(
        result.is_ok(),
        "async schema initialization failed: {:?}",
        result
    );
}
