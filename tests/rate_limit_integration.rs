use xavier::agents::rate_limit::RateLimitManager;

async fn setup_manager() -> (RateLimitManager, String) {
    let project_id = format!("test_metrics_{}", uuid::Uuid::new_v4().simple());
    let manager = RateLimitManager::new_with_project(&project_id);
    manager.init_schema_async().await.unwrap();
    (manager, project_id)
}

#[tokio::test]
async fn test_is_quota_low() {
    let (manager, _project_id) = setup_manager().await;
    let provider = "test-provider";

    assert!(!manager.is_quota_low(provider).await.unwrap());

    manager
        .track_request(provider, 910_000, 200, 0.0, false)
        .await
        .unwrap();

    // Wait for background DB writes
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    assert!(manager.is_quota_low(provider).await.unwrap());

    let (manager, _project_id) = setup_manager().await;
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
    let (manager, project_id) = setup_manager().await;
    let provider = "test-provider";

    xavier::codebase::connection_manager::ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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

    let (manager, project_id) = setup_manager().await;
    xavier::codebase::connection_manager::ConnectionManager::global()
        .with_conn(&project_id, move |conn| {
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
    let (manager, _project_id) = setup_manager().await;
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
