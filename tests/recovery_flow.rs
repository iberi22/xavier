use axum::{
    body::Body,
    http::{self, Request, StatusCode},
};
use chrono::Utc;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::net::TcpListener;
use tower::ServiceExt;
use uuid::Uuid;

use xavier::security::recovery::RecoverySystem as RecoveryManager;
use xavier::security::user_store::{User, UserStore};

// Helper function to create a test app
async fn setup_test_app() -> axum::Router {
    // Setup environment for tests
    std::env::set_var("XAVIER_TOKEN", "test-token");

    // In a real integration test we'd spawn the server,
    // but here we can just build the router.
    // We need a CliState.

    // This is complex because start_http_server does a lot.
    // Let's try to mock the minimal state.

    // For now, I'll focus on unit testing the logic and a simpler integration test if possible.
    // Actually, I can use the existing server routes if I can initialize the state.

    // Let's just do a manual test of the store and recovery manager first in a separate test file.

    panic!("Use unit tests and simplified integration tests instead of full server boot for now");
}

#[tokio::test]
async fn test_full_recovery_flow() {
    let project_id = format!("test_recovery_{}", Uuid::new_v4().simple());
    let user_store = UserStore::with_project_id(project_id.clone());

    // 1. Setup connection for test
    xavier::codebase::connection_manager::ConnectionManager::global()
        .connect(&project_id, ".")
        .unwrap();

    // Run migrations
    xavier::codebase::connection_manager::ConnectionManager::global()
        .with_conn(&project_id, |conn| {
            let mut manager = xavier::storage::MigrationManager::new();
            manager.add_migration(xavier::storage::migrations::MigrationV6RecoverySystem);
            manager.run_migrations(conn).unwrap();
            Ok(())
        })
        .await
        .unwrap();

    // 2. Create a user
    let email = "test@example.com";
    let password = "password123";
    let security_mgr = xavier::security::SecurityManager::new();
    let password_hash = security_mgr.hash_password(password).unwrap();
    let seed_phrase = RecoveryManager::generate_seed_phrase().unwrap();
    let seed_hash = RecoveryManager::hash_seed_phrase(&seed_phrase);

    let user = User {
        id: Uuid::new_v4().to_string(),
        email: email.to_string(),
        password_hash,
        recovery_seed_hash: seed_hash,
        two_factor_enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    user_store.add_user(user.clone()).await.unwrap();

    // 3. Verify seed phrase
    let provided_seed = seed_phrase.clone();
    let verified_hash = RecoveryManager::hash_seed_phrase(&provided_seed);
    assert_eq!(user.recovery_seed_hash, verified_hash);

    // 4. Reset password
    let new_password = "newpassword456";
    let new_password_hash = security_mgr.hash_password(new_password).unwrap();
    let new_seed = RecoveryManager::generate_seed_phrase().unwrap();
    let new_seed_hash = RecoveryManager::hash_seed_phrase(&new_seed);

    user_store
        .update_password_and_recovery(&user.id, &new_password_hash, &new_seed_hash)
        .await
        .unwrap();

    // 5. Check updated user
    let updated_user = user_store.get_user_by_email(email).await.unwrap().unwrap();
    assert!(security_mgr
        .verify_password(new_password, &updated_user.password_hash)
        .unwrap());
    assert_eq!(updated_user.recovery_seed_hash, new_seed_hash);
    assert!(!updated_user.two_factor_enabled); // 2FA should be disabled

    // 6. Backup codes
    let codes = RecoveryManager::generate_backup_codes();
    let mut backup_codes = Vec::new();
    for code in &codes {
        backup_codes.push(xavier::security::user_store::BackupCode {
            id: Uuid::new_v4().to_string(),
            user_id: user.id.clone(),
            code_hash: RecoveryManager::hash_backup_code(code),
            used: false,
        });
    }
    user_store.save_backup_codes(backup_codes).await.unwrap();

    assert_eq!(
        user_store
            .count_remaining_backup_codes(&user.id)
            .await
            .unwrap(),
        10
    );

    // Use a backup code
    let first_code = &codes[0];
    let first_code_hash = RecoveryManager::hash_backup_code(first_code);
    let success = user_store
        .verify_and_consume_backup_code(&user.id, &first_code_hash)
        .await
        .unwrap();
    assert!(success);

    assert_eq!(
        user_store
            .count_remaining_backup_codes(&user.id)
            .await
            .unwrap(),
        9
    );

    // Try to use same code again
    let success_again = user_store
        .verify_and_consume_backup_code(&user.id, &first_code_hash)
        .await
        .unwrap();
    assert!(!success_again);
}
