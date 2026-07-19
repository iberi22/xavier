use std::sync::Arc;
use xavier::coordination::KeyLendingEngine;
use xavier::secrets::audit::QmdAuditLogger;

#[tokio::test]
async fn test_secrets_redaction_serialization() -> anyhow::Result<()> {
    // 1. Setup minimal environment
    let temp_dir = tempfile::tempdir()?;
    std::env::set_var("XAVIER_DATA_DIR", temp_dir.path());

    // Initialize metrics DB for audit logging
    xavier::codebase::connection_manager::ConnectionManager::global()
        .connect("metrics", temp_dir.path().to_str().unwrap())?;
    let logger = QmdAuditLogger::new();
    logger.init_schema_async().await?;

    // 2. Mock engine
    let secrets_engine = Arc::new(KeyLendingEngine::new(Box::new(QmdAuditLogger::new()), None));

    // 3. Create a lease
    let mut lease = secrets_engine
        .lend("test-key", Some("secret-value"), "agent-1", 60)
        .await?;

    // 4. Verify it HAS the value initially (internal state)
    assert_eq!(lease.secret_value.as_deref(), Some("secret-value"));

    // 5. Redact (simulating what the handler does)
    lease.secret_value = None;

    // 6. Verify serialization skips it
    let serialized = serde_json::to_value(&lease)?;
    assert!(
        serialized.get("secret_value").is_none(),
        "secret_value should be missing from JSON when None"
    );

    Ok(())
}
