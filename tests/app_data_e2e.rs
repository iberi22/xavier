//! End-to-End Tests for `app_data_enc` Persistence (`tests/app_data_e2e.rs`)
//!
//! Connects to real Postgres database if `XAVIER_POSTGRES_URL` or `DATABASE_URL` is set,
//! otherwise skips external DB operations gracefully.

use chrono::Utc;
use sqlx::postgres::PgPoolOptions;
use xavier::crypto::hex_encode;
use xavier::nodes::byo::app_data::{
    AppDataManager, AppDataRecord, decode_bytea_str,
};

#[test]
fn test_decode_bytea_str_formats() {
    let raw = b"Hello, World!";
    let hex_raw = hex_encode(raw);

    // Test \\x prefix
    let with_slash_x = format!("\\x{}", hex_raw);
    assert_eq!(decode_bytea_str(&with_slash_x), raw);

    // Test 0x prefix
    let with_0x = format!("0x{}", hex_raw);
    assert_eq!(decode_bytea_str(&with_0x), raw);

    // Test bare hex
    assert_eq!(decode_bytea_str(&hex_raw), raw);

    // Test fallback plain string
    assert_eq!(decode_bytea_str("plain_string"), b"plain_string");
}

#[tokio::test]
async fn test_app_data_e2e_roundtrip_and_isolation() {
    let pg_url = std::env::var("XAVIER_POSTGRES_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok();

    let pg_url = match pg_url {
        Some(url) if !url.is_empty() => url,
        _ => {
            println!("Skipping Postgres E2E integration test: XAVIER_POSTGRES_URL / DATABASE_URL not set");
            return;
        }
    };

    let pool = match PgPoolOptions::new()
        .max_connections(5)
        .connect(&pg_url)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            println!("Skipping Postgres E2E test due to connection error: {}", e);
            return;
        }
    };

    let aes_key = [42u8; 32];
    let manager = AppDataManager::with_aes_key(pool, aes_key);

    manager
        .init_schema()
        .await
        .expect("Failed to initialize app_data_enc schema");

    let tenant_a = format!("tenant_a_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let tenant_b = format!("tenant_b_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let app_id = "swal-test-app";
    let kind = "config";
    let record_id = "user_settings_1";

    let original_payload = b"byte-exact-secret-payload-\x00\x01\x02\xFF-data";
    let record_a = AppDataRecord {
        tenant_id: tenant_a.clone(),
        app_id: app_id.to_string(),
        kind: kind.to_string(),
        id: record_id.to_string(),
        payload: original_payload.to_vec(),
        metadata: serde_json::json!({"version": 1, "owner": "alice"}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // 1. Put record
    manager
        .put(&record_a)
        .await
        .expect("Failed to put record for tenant A");

    // 2. Get record (byte-exact roundtrip verification)
    let fetched = manager
        .get(&tenant_a, app_id, kind, record_id)
        .await
        .expect("Failed to get record for tenant A")
        .expect("Record for tenant A should exist");

    assert_eq!(fetched.payload, original_payload);
    assert_eq!(fetched.tenant_id, tenant_a);
    assert_eq!(fetched.app_id, app_id);
    assert_eq!(fetched.kind, kind);
    assert_eq!(fetched.id, record_id);
    assert_eq!(fetched.metadata["owner"], "alice");

    // 3. Cross-tenant read attempt (isolation check)
    let cross_tenant_fetched = manager
        .get(&tenant_b, app_id, kind, record_id)
        .await
        .expect("Cross tenant query execution should succeed");
    assert!(
        cross_tenant_fetched.is_none(),
        "Cross-tenant read attempt must return None"
    );

    // 4. List by kind
    let record_a2 = AppDataRecord {
        tenant_id: tenant_a.clone(),
        app_id: app_id.to_string(),
        kind: kind.to_string(),
        id: "user_settings_2".to_string(),
        payload: b"second-payload".to_vec(),
        metadata: serde_json::json!({"version": 2}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    manager.put(&record_a2).await.expect("Failed to put record A2");

    let list_a = manager
        .list_by_kind(&tenant_a, app_id, kind)
        .await
        .expect("Failed to list records by kind for tenant A");
    assert_eq!(list_a.len(), 2);
    assert_eq!(list_a[0].id, record_id);
    assert_eq!(list_a[1].id, "user_settings_2");

    let list_b = manager
        .list_by_kind(&tenant_b, app_id, kind)
        .await
        .expect("List by kind for tenant B should succeed");
    assert!(list_b.is_empty(), "Tenant B should have zero records");

    // 5. Delete record
    let deleted = manager
        .delete(&tenant_a, app_id, kind, record_id)
        .await
        .expect("Failed to delete record A");
    assert!(deleted, "Delete should return true for existing record");

    let post_delete = manager
        .get(&tenant_a, app_id, kind, record_id)
        .await
        .expect("Fetch post-delete should succeed");
    assert!(post_delete.is_none(), "Record should be deleted");

    // Clean up second record
    manager
        .delete(&tenant_a, app_id, kind, "user_settings_2")
        .await
        .ok();
}
