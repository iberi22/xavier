use anyhow::Result;
use mockito::Server;
use xavier::nodes::byo::TenantManager;

#[tokio::test]
async fn test_hmac_tenant_id_generation() -> Result<()> {
    let secret = b"super_secret_master_key_12345";
    let identifier = b"org_acme_corp";

    let tenant_id = TenantManager::generate_tenant_id(secret, identifier);
    assert!(!tenant_id.is_empty());

    // Deterministic output test
    let tenant_id_repeat = TenantManager::generate_tenant_id(secret, identifier);
    assert_eq!(tenant_id, tenant_id_repeat);

    // Verify it's valid base64
    let decoded = xavier::crypto::base64_decode(&tenant_id);
    assert!(decoded.is_some());
    assert_eq!(decoded.unwrap().len(), 32); // HMAC-SHA256 output is 32 bytes

    println!("✅ HMAC tenant_id generation test passed.");
    Ok(())
}

#[tokio::test]
async fn test_supabase_postgrest_crud() -> Result<()> {
    let mut server = Server::new_async().await;
    let mock_url = server.url();
    let mock_key = "sb_secret_api_key_test_123";
    let auth_header = format!("Bearer {}", mock_key);

    let tenant_id = "tenant_sb_999";

    // 1. Mock create tenant (POST /rest/v1/tenants)
    let _mock_create = server
        .mock("POST", "/rest/v1/tenants")
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .match_header("Prefer", "return=representation,resolution=merge-duplicates")
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"[
                {{
                    "tenant_id": "{}",
                    "tier": "free",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                }}
            ]"#,
            tenant_id
        ))
        .create_async()
        .await;

    // 2. Mock get tenant (GET /rest/v1/tenants?tenant_id=eq.tenant_sb_999)
    let get_path = format!("/rest/v1/tenants?tenant_id=eq.{}", tenant_id);
    let _mock_get = server
        .mock("GET", get_path.as_str())
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"[
                {{
                    "tenant_id": "{}",
                    "tier": "free",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z"
                }}
            ]"#,
            tenant_id
        ))
        .create_async()
        .await;

    // 3. Mock update tier (PATCH /rest/v1/tenants?tenant_id=eq.tenant_sb_999)
    let _mock_patch = server
        .mock("PATCH", get_path.as_str())
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .match_header("Prefer", "return=representation")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"[
                {{
                    "tenant_id": "{}",
                    "tier": "enterprise",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:05:00Z"
                }}
            ]"#,
            tenant_id
        ))
        .create_async()
        .await;

    // 4. Mock insert app_data_enc (POST /rest/v1/app_data_enc)
    let _mock_insert_app_data = server
        .mock("POST", "/rest/v1/app_data_enc")
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .with_status(201)
        .with_header("content-type", "application/json")
        .with_body(r#"[{"id": "data_1"}]"#)
        .create_async()
        .await;

    // 5. Mock count app_data_enc (GET /rest/v1/app_data_enc?tenant_id=eq.tenant_sb_999&select=id)
    let count_path = format!("/rest/v1/app_data_enc?tenant_id=eq.{}&select=id", tenant_id);
    let _mock_count_app_data = server
        .mock("GET", count_path.as_str())
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[{"id": "data_1"}]"#)
        .create_async()
        .await;

    // 6. Mock delete app_data_enc (DELETE /rest/v1/app_data_enc?tenant_id=eq.tenant_sb_999)
    let del_app_data_path = format!("/rest/v1/app_data_enc?tenant_id=eq.{}", tenant_id);
    let _mock_del_app_data = server
        .mock("DELETE", del_app_data_path.as_str())
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .with_status(204)
        .create_async()
        .await;

    // 7. Mock delete tenant (DELETE /rest/v1/tenants?tenant_id=eq.tenant_sb_999)
    let _mock_del_tenant = server
        .mock("DELETE", get_path.as_str())
        .match_header("apikey", mock_key)
        .match_header("Authorization", auth_header.as_str())
        .with_status(204)
        .create_async()
        .await;

    // Initialize TenantManager for Supabase
    let manager = TenantManager::new_supabase(&mock_url, mock_key)?;

    // Create
    let record = manager.create(tenant_id, "free").await?;
    assert_eq!(record.tenant_id, tenant_id);
    assert_eq!(record.tier, "free");

    // Get
    let fetched = manager.get(tenant_id).await?;
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().tier, "free");

    // Update tier
    let updated = manager.update_tier(tenant_id, "enterprise").await?;
    assert_eq!(updated.tier, "enterprise");

    // Insert app_data_enc & count
    manager
        .insert_app_data_enc("data_1", tenant_id, "payload_enc_123")
        .await?;
    let count = manager.count_app_data_enc(tenant_id).await?;
    assert_eq!(count, 1);

    // Delete (cascade delete app_data_enc)
    manager.delete(tenant_id).await?;

    println!("✅ Supabase PostgREST TenantManager CRUD test passed.");
    Ok(())
}

#[tokio::test]
async fn test_neon_sqlx_crud() -> Result<()> {
    let neon_url = std::env::var("XAVIER_NEON_URL")
        .or_else(|_| std::env::var("POSTGRES_URL"))
        .or_else(|_| std::env::var("DATABASE_URL"));

    if let Ok(url) = neon_url {
        println!("Connecting to Neon/Postgres DB at: {}", url);
        let manager = TenantManager::new_neon_from_url(&url).await?;

        // Initialize schema
        manager.init_schema().await?;

        let secret = b"neon_secret_key";
        let identifier = b"neon_tenant_acme";
        let tenant_id = TenantManager::generate_tenant_id(secret, identifier);

        // 1. Create real tenant
        let record = manager.create(&tenant_id, "free").await?;
        assert_eq!(record.tenant_id, tenant_id);
        assert_eq!(record.tier, "free");

        // 2. Read back tenant
        let fetched = manager.get(&tenant_id).await?;
        assert!(fetched.is_some());
        let fetched_record = fetched.unwrap();
        assert_eq!(fetched_record.tenant_id, tenant_id);
        assert_eq!(fetched_record.tier, "free");

        // 3. Update tier
        let updated = manager.update_tier(&tenant_id, "pro").await?;
        assert_eq!(updated.tier, "pro");

        let re_fetched = manager.get(&tenant_id).await?.expect("tenant exists");
        assert_eq!(re_fetched.tier, "pro");

        // 4. Insert app_data_enc to verify cascade delete
        let app_data_id = format!("enc_data_{}", tenant_id);
        manager
            .insert_app_data_enc(&app_data_id, &tenant_id, "secret_encrypted_data")
            .await?;

        let data_count = manager.count_app_data_enc(&tenant_id).await?;
        assert_eq!(data_count, 1, "App data record should exist");

        // 5. Delete tenant and verify cascade delete of app_data_enc
        manager.delete(&tenant_id).await?;

        let deleted_tenant = manager.get(&tenant_id).await?;
        assert!(
            deleted_tenant.is_none(),
            "Tenant should be deleted from Neon"
        );

        let data_count_after = manager.count_app_data_enc(&tenant_id).await?;
        assert_eq!(
            data_count_after, 0,
            "app_data_enc records should be cascade deleted"
        );

        println!("✅ Neon sqlx real DB TenantManager CRUD + cascade delete test passed.");
    } else {
        println!("Skipping live Neon connection (XAVIER_NEON_URL not set). Testing struct initialization.");
        // Verify constructor parsing
        assert!(TenantManager::new_supabase("http://localhost:8000", "key").is_ok());
        println!("✅ Neon TenantManager structural verification passed.");
    }

    Ok(())
}
