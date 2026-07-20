use xavier::security::auth::{
    generate_jwt, validate_jwt, Permission, User, UserRole,
};

#[test]
fn test_user_roles_initialization() {
    let admin = User::new(
        "admin@swal.dev".to_string(),
        "Super Admin".to_string(),
        UserRole::Admin,
    );
    assert_eq!(admin.role, UserRole::Admin);
    assert!(admin.email.contains("admin"));
    assert!(admin.api_key.starts_with("sk-"));

    let user = User::new(
        "user@swal.dev".to_string(),
        "Regular User".to_string(),
        UserRole::User,
    );
    assert_eq!(user.role, UserRole::User);

    let readonly = User::new(
        "readonly@swal.dev".to_string(),
        "Readonly User".to_string(),
        UserRole::Readonly,
    );
    assert_eq!(readonly.role, UserRole::Readonly);
}

#[test]
fn test_permission_trait_matrix() {
    // 1. Admin Permissions
    let admin = UserRole::Admin;
    assert!(admin.can_view_dashboard());
    assert!(admin.can_search_memory());
    assert!(admin.can_add_memory());
    assert!(admin.can_delete_memory());
    assert!(admin.can_manage_beliefs());
    assert!(admin.can_run_agents());
    assert!(admin.can_view_config());
    assert!(admin.can_edit_config());
    assert!(admin.can_manage_users());

    // 2. Regular User Permissions
    let user = UserRole::User;
    assert!(user.can_view_dashboard());
    assert!(user.can_search_memory());
    assert!(user.can_add_memory());
    assert!(user.can_delete_memory());
    assert!(user.can_manage_beliefs());
    assert!(user.can_run_agents());
    assert!(user.can_view_config());
    assert!(!user.can_edit_config());
    assert!(!user.can_manage_users());

    // 3. Readonly User Permissions
    let readonly = UserRole::Readonly;
    assert!(readonly.can_view_dashboard());
    assert!(readonly.can_search_memory());
    assert!(!readonly.can_add_memory());
    assert!(!readonly.can_delete_memory());
    assert!(!readonly.can_manage_beliefs());
    assert!(!readonly.can_run_agents());
    assert!(readonly.can_view_config());
    assert!(!readonly.can_edit_config());
    assert!(!readonly.can_manage_users());
}

#[test]
fn test_jwt_claims_role_propagation() {
    // Install default crypto provider for jsonwebtoken
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();

    let secret = b"my_super_secret_for_rbac_tests_2026";

    let roles = vec![UserRole::Admin, UserRole::User, UserRole::Readonly];

    for role in roles {
        let user = User::new(
            format!("{:?}@swal.dev", role).to_lowercase(),
            format!("{:?} User", role),
            role,
        );

        let token = generate_jwt(&user, secret).expect("JWT token generation failed");
        let claims = validate_jwt(&token, secret).expect("JWT validation failed");

        assert_eq!(claims.sub, user.id);
        assert_eq!(claims.email, user.email);
        assert_eq!(claims.role, role);
    }
}

#[test]
fn test_jwt_validation_invalid_secret() {
    // Install default crypto provider for jsonwebtoken
    let _ = jsonwebtoken::crypto::rust_crypto::DEFAULT_PROVIDER.install_default();

    let secret = b"correct_secret_key";
    let wrong_secret = b"wrong_secret_key";

    let user = User::new(
        "test@swal.dev".to_string(),
        "Test User".to_string(),
        UserRole::User,
    );

    let token = generate_jwt(&user, secret).expect("JWT token generation failed");
    let validation_result = validate_jwt(&token, wrong_secret);

    assert!(validation_result.is_err());
}
