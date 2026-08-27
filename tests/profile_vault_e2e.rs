//! E2E Integration test for ProfileVault encrypted API (Postgres / Neon)
//!
//! Verifies:
//! 1. Table initialization (`profile_vault_enc`).
//! 2. Roundtrip save & load of UserProfile (identical JSON).
//! 3. Ciphertext security in DB: raw ciphertext stored in DB does NOT contain plaintext email or name.
//! 4. Single-tenant upsert semantics.

use xavier::nodes::byo::{ProfileVault, UserProfile, HKDF_PROFILE_VAULT_INFO};
use xavier::security::encryption_keys::MasterKeyManager;

fn get_db_url() -> Option<String> {
    std::env::var("NEON_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .or_else(|_| std::env::var("XAVIER_POSTGRES_URL"))
        .ok()
}

#[tokio::test]
async fn test_profile_vault_e2e() -> Result<(), Box<dyn std::error::Error>> {
    let db_url = match get_db_url() {
        Some(url) if !url.trim().is_empty() => url,
        _ => {
            println!(
                "Skipping profile_vault_e2e test: No Postgres connection URL set in NEON_DATABASE_URL, DATABASE_URL, or XAVIER_POSTGRES_URL"
            );
            return Ok(());
        }
    };

    println!("Connecting to Postgres/Neon at {}", db_url);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let master_key_mgr = MasterKeyManager::load_or_init()?;
    let vault = ProfileVault::new(pool.clone(), &master_key_mgr)?;

    // 0. Verify HKDF tag contract
    assert_eq!(HKDF_PROFILE_VAULT_INFO, b"swal-profile-vault-v1");

    // 1. Initialize DB schema
    vault.init_table().await?;

    let test_tenant_id = format!("tenant-e2e-{}", rand::random::<u64>());
    let original_profile = UserProfile {
        tenant_id: test_tenant_id.clone(),
        email: "security-user@swal.dev".to_string(),
        name: "Security Tester".to_string(),
        preferences: serde_json::json!({
            "theme": "dark",
            "locale": "en_US",
            "notifications": true
        }),
    };

    // 2. Roundtrip test: Save profile -> Load -> Identical JSON
    vault.save_profile(&original_profile).await?;
    let loaded_profile = vault
        .get_profile(&test_tenant_id)
        .await?
        .expect("Profile should exist in vault");

    assert_eq!(
        original_profile, loaded_profile,
        "Loaded profile must be identical to saved profile"
    );

    // 3. Ciphertext security check via direct SQL
    let raw_ciphertext = vault
        .get_raw_ciphertext(&test_tenant_id)
        .await?
        .expect("Raw ciphertext should exist in DB");

    // Also perform direct raw SQL query to verify DB column content
    let db_row = sqlx::query("SELECT ciphertext FROM profile_vault_enc WHERE tenant_id = $1")
        .bind(&test_tenant_id)
        .fetch_one(&pool)
        .await?;
    let direct_ciphertext: Vec<u8> = sqlx::Row::get(&db_row, "ciphertext");

    assert_eq!(raw_ciphertext, direct_ciphertext);

    let email_bytes = original_profile.email.as_bytes();
    let name_bytes = original_profile.name.as_bytes();

    let contains_email = direct_ciphertext
        .windows(email_bytes.len())
        .any(|window| window == email_bytes);
    let contains_name = direct_ciphertext
        .windows(name_bytes.len())
        .any(|window| window == name_bytes);

    assert!(
        !contains_email,
        "Ciphertext in DB must NOT contain plaintext email!"
    );
    assert!(
        !contains_name,
        "Ciphertext in DB must NOT contain plaintext name!"
    );

    // 4. Test Upsert semantics (update profile for same tenant)
    let updated_profile = UserProfile {
        tenant_id: test_tenant_id.clone(),
        email: "security-user-updated@swal.dev".to_string(),
        name: "Security Tester Updated".to_string(),
        preferences: serde_json::json!({
            "theme": "light",
            "locale": "es_ES",
            "notifications": false
        }),
    };

    vault.save_profile(&updated_profile).await?;
    let reloaded = vault
        .get_profile(&test_tenant_id)
        .await?
        .expect("Updated profile should exist");

    assert_eq!(reloaded, updated_profile);
    assert_ne!(reloaded, original_profile);

    // Cleanup test record
    vault.delete_profile(&test_tenant_id).await?;
    assert!(vault.get_profile(&test_tenant_id).await?.is_none());

    println!("ProfileVault E2E test completed successfully.");
    Ok(())
}
