//! E2E tests for BYO Persistence Node Billing Records API (`src/nodes/byo/billing.rs`).
//!
//! Verifies full subscription billing lifecycle:
//! Create -> get_active -> cancel -> list_history against Postgres / Neon (`DATABASE_URL`).
//! Also verifies no plaintext PII (payment_ref hashed as opaque SHA-256) and NUMERIC(12,2) storage.

use sqlx::postgres::PgPoolOptions;
use std::env;
use xavier::nodes::byo::billing::{BillingManager, BillingPlan};
use xavier::utils::crypto::sha256_hex;

#[tokio::test]
async fn test_billing_records_lifecycle() {
    let db_url = match env::var("DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => {
            println!("Skipping test_billing_records_lifecycle: DATABASE_URL not set");
            return;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    let manager = BillingManager::new(pool);

    // 1. Initialize schema
    manager
        .init_schema()
        .await
        .expect("Schema initialization failed");

    let tenant_id = format!("tenant_{}", uuid::Uuid::new_v4());
    let raw_payment_ref = "inv_invoice_12345_sensitive_user@example.com";
    let expected_hash = sha256_hex(raw_payment_ref.as_bytes());

    // Initially, active record should be None
    let active_none = manager
        .get_active(&tenant_id)
        .await
        .expect("get_active failed");
    assert!(active_none.is_none());

    // 2. Create record: Socio plan ($8.00 USDC)
    let record = manager
        .create_record(&tenant_id, BillingPlan::Socio, "2025-01", raw_payment_ref)
        .await
        .expect("create_record failed");

    assert_eq!(record.tenant_id, tenant_id);
    assert_eq!(record.plan, BillingPlan::Socio);
    assert_eq!(record.amount_usdc, "8.00");
    assert_eq!(record.period, "2025-01");
    assert_eq!(record.status, "active");
    // Ensure no plaintext PII: payment_ref must equal expected SHA-256 hash
    assert_eq!(record.payment_ref, expected_hash);
    assert!(!record.payment_ref.contains("user@example.com"));

    // 3. get_active returns created record
    let active = manager
        .get_active(&tenant_id)
        .await
        .expect("get_active failed")
        .expect("Active subscription missing");

    assert_eq!(active.billing_id, record.billing_id);
    assert_eq!(active.status, "active");
    assert_eq!(active.amount_usdc, "8.00");
    assert_eq!(active.payment_ref, expected_hash);

    // 4. Cancel active subscription
    let cancelled = manager
        .cancel(&record.billing_id)
        .await
        .expect("cancel failed");

    assert_eq!(cancelled.billing_id, record.billing_id);
    assert_eq!(cancelled.status, "cancelled");

    // get_active should now return None
    let active_after_cancel = manager
        .get_active(&tenant_id)
        .await
        .expect("get_active after cancel failed");
    assert!(active_after_cancel.is_none());

    // 5. Create a new subscription: Micro plan ($3.00 USDC)
    let new_record = manager
        .create_record(&tenant_id, BillingPlan::Micro, "2025-02", "tx_hash_999888")
        .await
        .expect("create_record second subscription failed");

    assert_eq!(new_record.plan, BillingPlan::Micro);
    assert_eq!(new_record.amount_usdc, "3.00");

    let active_new = manager
        .get_active(&tenant_id)
        .await
        .expect("get_active new failed")
        .expect("Active subscription missing");
    assert_eq!(active_new.billing_id, new_record.billing_id);

    // 6. list_history returns both records ordered by created_at DESC
    let history = manager
        .list_history(&tenant_id)
        .await
        .expect("list_history failed");

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].billing_id, new_record.billing_id);
    assert_eq!(history[0].status, "active");
    assert_eq!(history[1].billing_id, record.billing_id);
    assert_eq!(history[1].status, "cancelled");
}

#[tokio::test]
async fn test_billing_plans_pricing() {
    assert_eq!(BillingPlan::Free.amount_usdc(), 0.0);
    assert_eq!(BillingPlan::Free.amount_usdc_str(), "0.00");

    assert_eq!(BillingPlan::Micro.amount_usdc(), 3.0);
    assert_eq!(BillingPlan::Micro.amount_usdc_str(), "3.00");

    assert_eq!(BillingPlan::Socio.amount_usdc(), 8.0);
    assert_eq!(BillingPlan::Socio.amount_usdc_str(), "8.00");

    assert_eq!(BillingPlan::Nodo.amount_usdc(), 15.0);
    assert_eq!(BillingPlan::Nodo.amount_usdc_str(), "15.00");
}
