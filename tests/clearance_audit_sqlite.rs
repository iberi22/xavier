use std::fs;
use tempfile::tempdir;

use serial_test::serial;
use xavier::codebase::connection_manager::ConnectionManager;
use xavier::security::audit::AuditLogger;
use xavier::security::clearance::ClearanceLevel;
use xavier::security::clearance_audit::{record_and_mirror, ClearanceReadAudit};

#[tokio::test]
#[serial]
async fn test_allowed_search_entry_mirrored_to_sqlite() {
    let temp = tempdir().unwrap();
    let project_id = "security";
    let db_dir = temp.path().to_string_lossy().to_string();

    ConnectionManager::global()
        .connect(project_id, &db_dir)
        .unwrap();

    let logger = AuditLogger::new();
    logger.init_schema().await.unwrap();

    let jsonl_file = temp.path().join("clearance_audit.jsonl");
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_PATH", &jsonl_file);
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_SQLITE", "1");

    let entry = ClearanceReadAudit::new(
        "usr-001",
        "analyst",
        ClearanceLevel::Confidential,
        "/v1/memory/search",
        "search",
        "confidential research",
        1,
        0,
        true,
    );

    record_and_mirror(entry).await.unwrap();

    let logs = logger.list_logs().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].user_id, "usr-001");
    assert_eq!(logs[0].role, "analyst");
    assert!(
        logs[0].permission.contains("clearance:search:"),
        "Permission should contain 'clearance:search:', got: {}",
        logs[0].permission
    );
    assert_eq!(logs[0].permission, "clearance:search:/v1/memory/search");
    assert_eq!(logs[0].result, "ALLOW");

    let jsonl_content = fs::read_to_string(&jsonl_file).unwrap();
    assert!(jsonl_content.contains("\"subject\":\"usr-001\""));
    assert!(jsonl_content.contains("\"allowed\":true"));

    ConnectionManager::global().disconnect(project_id);
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_PATH");
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_SQLITE");
}

#[tokio::test]
#[serial]
async fn test_denied_route_entry_mirrored_with_deny_result() {
    let temp = tempdir().unwrap();
    let project_id = "security";
    let db_dir = temp.path().to_string_lossy().to_string();

    ConnectionManager::global()
        .connect(project_id, &db_dir)
        .unwrap();

    let logger = AuditLogger::new();
    logger.init_schema().await.unwrap();

    let jsonl_file = temp.path().join("clearance_audit.jsonl");
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_PATH", &jsonl_file);
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_SQLITE", "1");

    let entry = ClearanceReadAudit::new(
        "usr-002",
        "guest",
        ClearanceLevel::Restricted,
        "/v1/classified/restricted",
        "read",
        "",
        0,
        1,
        false,
    );

    record_and_mirror(entry).await.unwrap();

    let logs = logger.list_logs().await.unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].user_id, "usr-002");
    assert_eq!(logs[0].role, "guest");
    assert_eq!(
        logs[0].permission,
        "clearance:read:/v1/classified/restricted"
    );
    assert_eq!(logs[0].result, "DENY");

    let jsonl_content = fs::read_to_string(&jsonl_file).unwrap();
    assert!(jsonl_content.contains("\"subject\":\"usr-002\""));
    assert!(jsonl_content.contains("\"allowed\":false"));
    assert!(jsonl_content.contains("\"hidden_by_clearance\":1"));

    ConnectionManager::global().disconnect(project_id);
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_PATH");
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_SQLITE");
}

#[tokio::test]
#[serial]
async fn test_sqlite_disabled_skips_mirror_but_writes_jsonl() {
    let temp = tempdir().unwrap();
    let project_id = "security";
    let db_dir = temp.path().to_string_lossy().to_string();

    ConnectionManager::global()
        .connect(project_id, &db_dir)
        .unwrap();

    let logger = AuditLogger::new();
    logger.init_schema().await.unwrap();

    let jsonl_file = temp.path().join("clearance_audit.jsonl");
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_PATH", &jsonl_file);
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_SQLITE", "0");

    let entry = ClearanceReadAudit::new(
        "usr-003",
        "operator",
        ClearanceLevel::Confidential,
        "/v1/memory/export",
        "export",
        "",
        1,
        0,
        true,
    );

    record_and_mirror(entry).await.unwrap();

    let logs = logger.list_logs().await.unwrap();
    assert_eq!(
        logs.len(),
        0,
        "SQLite mirror should be skipped when XAVIER_CLEARANCE_AUDIT_SQLITE=0"
    );

    let jsonl_content = fs::read_to_string(&jsonl_file).unwrap();
    assert!(
        jsonl_content.contains("\"subject\":\"usr-003\""),
        "JSONL file should still be written"
    );

    ConnectionManager::global().disconnect(project_id);
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_PATH");
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_SQLITE");
}

#[tokio::test]
#[serial]
async fn test_uninitialized_sqlite_fails_open() {
    let temp = tempdir().unwrap();

    // Disconnect security project if connected
    ConnectionManager::global().disconnect("security");

    let jsonl_file = temp.path().join("clearance_audit_uninit.jsonl");
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_PATH", &jsonl_file);
    std::env::set_var("XAVIER_CLEARANCE_AUDIT_SQLITE", "1");

    let entry = ClearanceReadAudit::new(
        "usr-004",
        "admin",
        ClearanceLevel::TopSecret,
        "/v1/admin/audit",
        "query",
        "",
        0,
        0,
        false,
    );

    // Call record_and_mirror - should fail-open without panicking or returning Err
    let result = record_and_mirror(entry).await;
    assert!(result.is_ok());

    let jsonl_content = fs::read_to_string(&jsonl_file).unwrap();
    assert!(jsonl_content.contains("\"subject\":\"usr-004\""));

    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_PATH");
    std::env::remove_var("XAVIER_CLEARANCE_AUDIT_SQLITE");
}
