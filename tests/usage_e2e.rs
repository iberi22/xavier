use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use xavier::nodes::byo::usage::UsageTracker;

#[tokio::test]
async fn test_usage_e2e() {
    let database_url = match std::env::var("DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => {
            eprintln!("DATABASE_URL not set; skipping usage_e2e test against PostgreSQL/Neon");
            return;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to Postgres/Neon DB");

    // Setup table schema
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS usage_metrics (
            tenant_id TEXT NOT NULL,
            month VARCHAR(7) NOT NULL,
            kind TEXT NOT NULL,
            units BIGINT NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW(),
            PRIMARY KEY (tenant_id, month, kind)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("Failed to create usage_metrics table");

    let tenant = format!("test_tenant_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let tracker = UsageTracker::new(pool.clone());
    let current_month = Utc::now().format("%Y-%m").to_string();

    // 1. Verify record() twice same month/kind increments not duplicates
    tracker.record(&tenant, "llm_completion", 20).await.expect("Failed to record usage");
    tracker.record(&tenant, "llm_completion", 15).await.expect("Failed to record usage again");

    let total_llm = tracker.monthly_total(&tenant, &current_month).await.expect("Failed to get monthly total");
    assert_eq!(total_llm, 35, "record() twice same month/kind should increment units to 35");

    // 2. Verify monthly_total returns correct sum across kinds
    tracker.record(&tenant, "embedding_generation", 10).await.expect("Failed to record embedding usage");

    let total_all = tracker.monthly_total(&tenant, &current_month).await.expect("Failed to get monthly total across kinds");
    assert_eq!(total_all, 45, "monthly_total should return sum across all kinds (35 + 10 = 45)");

    // 3. Verify check_quota correctly compares against plan limits
    // Current total is 45.
    // free limit = 50 -> 45 < 50 => true
    assert!(tracker.check_quota(&tenant, "free").await.expect("Quota check free failed"));

    // Record 10 more units -> total 55
    tracker.record(&tenant, "llm_completion", 10).await.expect("Failed to record additional usage");
    let total_55 = tracker.monthly_total(&tenant, &current_month).await.expect("Failed to get total");
    assert_eq!(total_55, 55);

    // free limit = 50 -> 55 < 50 => false
    assert!(!tracker.check_quota(&tenant, "free").await.expect("Quota check free over limit failed"));

    // micro limit = 400 -> 55 < 400 => true
    assert!(tracker.check_quota(&tenant, "micro").await.expect("Quota check micro failed"));

    // socio limit = 1500 -> 55 < 1500 => true
    assert!(tracker.check_quota(&tenant, "socio").await.expect("Quota check socio failed"));

    // nodo limit = unlimited -> always true
    assert!(tracker.check_quota(&tenant, "nodo").await.expect("Quota check nodo failed"));

    // Clean up test tenant data
    sqlx::query("DELETE FROM usage_metrics WHERE tenant_id = $1")
        .bind(&tenant)
        .execute(&pool)
        .await
        .expect("Failed to clean up test tenant usage");
}
