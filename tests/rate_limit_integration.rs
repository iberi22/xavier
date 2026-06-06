use xavier::agents::rate_limit::RateLimitManager;
use xavier::ports::outbound::schema_init::SchemaInitializer;

async fn setup_manager() -> (RateLimitManager, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    std::env::set_var("XAVIER_DATA_DIR", temp_dir.path());
    xavier::codebase::connection_manager::ConnectionManager::global().disconnect("metrics");
    let manager = RateLimitManager::new();
    manager.init_schema_async().await.unwrap();
    (manager, temp_dir)
}

#[tokio::test]
async fn test_is_quota_low() {
    let (manager, _tmp) = setup_manager().await;
    let provider = "test-provider";

    assert!(!manager.is_quota_low(provider).await.unwrap());

    manager
        .track_request(provider, 910_000, 200, 0.0, false)
        .await
        .unwrap();

    // Wait for background DB writes
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    assert!(manager.is_quota_low(provider).await.unwrap());

    let (manager, _tmp) = setup_manager().await;
    manager
        .track_request(provider, 890_000, 200, 0.0, false)
        .await
        .unwrap();

    // Wait for background DB writes
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    assert!(!manager.is_quota_low(provider).await.unwrap());
}

#[tokio::test]
async fn test_custom_weekly_quota() {
    let (manager, _tmp) = setup_manager().await;
    let provider = "test-provider";

    xavier::codebase::connection_manager::ConnectionManager::global()
        .with_conn("metrics", move |conn| {
            conn.execute(
                "INSERT INTO provider_quotas (provider, weekly_quota) VALUES (?1, ?2)",
                (provider.to_string(), 1000i64),
            )?;
            Ok(())
        })
        .await
        .unwrap();

    manager
        .track_request(provider, 950, 200, 0.0, false)
        .await
        .unwrap();

    // Wait for background DB writes
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    assert!(manager.is_quota_low(provider).await.unwrap());

    let (manager, _tmp) = setup_manager().await;
    xavier::codebase::connection_manager::ConnectionManager::global()
        .with_conn("metrics", move |conn| {
            conn.execute(
                "INSERT INTO provider_quotas (provider, weekly_quota) VALUES (?1, ?2)",
                (provider.to_string(), 1000i64),
            )?;
            Ok(())
        })
        .await
        .unwrap();
    manager
        .track_request(provider, 850, 200, 0.0, false)
        .await
        .unwrap();

    // Wait for background DB writes
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    assert!(!manager.is_quota_low(provider).await.unwrap());
}

#[tokio::test]
async fn test_daily_summary_and_hourly_usage() {
    let (manager, _tmp) = setup_manager().await;
    let provider = "test-provider";

    manager
        .track_request(provider, 100, 200, 0.01, false)
        .await
        .unwrap();
    manager
        .track_request(provider, 200, 200, 0.02, false)
        .await
        .unwrap();

    // Wait for background DB writes
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    let status = manager.get_status(provider).await.unwrap();
    assert_eq!(status.used_hourly, 300);
    assert_eq!(status.used_today, 300);

    let summary = manager.get_daily_summary(provider).await.unwrap();
    assert_eq!(summary["daily_total"], 2);
    assert_eq!(summary["daily_tokens"], 300);
    assert!(summary["requests"].as_array().unwrap().len() == 2);
}
