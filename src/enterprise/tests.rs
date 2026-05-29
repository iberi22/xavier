//! Enterprise feature unit tests
//!
//! Tests for tenant CRUD, API key management, rate limiting, and audit logging.
//! All tests are gated behind `#[cfg(test)]`.

use crate::enterprise::{
    audit::{AuditAction, AuditEntry, AuditLog},
    keys::{ApiKeyStore, ApiKeyType},
    rate_limit::{RateLimitConfig, RateLimitKey, RateLimiter},
    tenancy::{Plan, TenantStore},
};
use uuid::Uuid;

// ─────────────────────────────────────────────────────────────────────────────
// Tenant CRUD Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_tenant_create_and_list() {
    let mut store = TenantStore::new();
    assert!(store.list().is_empty());

    let tenant = store.create("test-tenant", Plan::Enterprise);
    assert_eq!(tenant.name, "test-tenant");
    assert_eq!(tenant.plan, Plan::Enterprise);
    assert!(store.exists(&tenant.id));

    let tenants = store.list();
    assert!(!tenants.is_empty());
}

#[test]
fn test_tenant_get_by_id() {
    let mut store = TenantStore::new();
    let tenant = store.create("get-test", Plan::Pro);

    let fetched = store.get(&tenant.id);
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().name, "get-test");
}

#[test]
fn test_tenant_get_nonexistent() {
    let store = TenantStore::new();
    let id = Uuid::new_v4();
    assert!(store.get(&id).is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// API Key Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_api_key_create_and_list() {
    let mut store = ApiKeyStore::new();
    let tenant_id = Uuid::new_v4();

    let (raw_key, key) = store.create(tenant_id, "test-key", ApiKeyType::Live);
    assert!(!raw_key.is_empty());
    assert_eq!(key.name, "test-key");
    assert_eq!(key.tenant_id, tenant_id);
    assert_eq!(key.key_type, ApiKeyType::Live);

    let keys = store.list_for_tenant(&tenant_id);
    assert_eq!(keys.len(), 1);
}

#[test]
fn test_api_key_validation() {
    let mut store = ApiKeyStore::new();
    let tenant_id = Uuid::new_v4();

    let (raw_key, _key) = store.create(tenant_id, "valid-key", ApiKeyType::Live);

    // Validate with correct key
    let valid = store.validate(&raw_key);
    assert!(valid.is_some());

    // Validate with wrong key
    let invalid = store.validate("wrong-key");
    assert!(invalid.is_none());
}

#[test]
fn test_api_key_revocation() {
    let mut store = ApiKeyStore::new();
    let tenant_id = Uuid::new_v4();

    let (_raw_key, key) = store.create(tenant_id, "revocable-key", ApiKeyType::Live);

    // Revoke the key — should succeed
    let result = store.revoke(&key.id);
    assert!(result.is_ok());

    // Key should still be listed but revoked
    let keys = store.list_for_tenant(&tenant_id);
    assert!(!keys.is_empty());
    assert!(keys.iter().any(|k| k.id == key.id && k.revoked));
}

// ─────────────────────────────────────────────────────────────────────────────
// Rate Limiter Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rate_limiter_allow_by_default() {
    let mut limiter = RateLimiter::new();
    let tenant_id = Uuid::new_v4();
    let key = RateLimitKey::Tenant(tenant_id);

    // Without any config, should be allowed
    let result = limiter.check(key);
    assert!(result.allowed);
}

#[test]
fn test_rate_limiter_tenant_specific() {
    let mut limiter = RateLimiter::new();
    let tenant_id = Uuid::new_v4();
    let other_id = Uuid::new_v4();
    let tenant_key = RateLimitKey::Tenant(tenant_id);
    let other_key = RateLimitKey::Tenant(other_id);

    // Set rate limit for the specific tenant (1 RPM)
    let config = RateLimitConfig::custom(1, 1);
    limiter.set_config(tenant_key.clone(), config);

    // First request for tenant should be allowed
    let first = limiter.check(tenant_key.clone());
    assert!(first.allowed, "first tenant request should be allowed");

    // Second request for tenant should be denied (1 RPM limit)
    let second = limiter.check(tenant_key.clone());
    assert!(!second.allowed, "second tenant request should be denied");

    // Other tenant should still be allowed (different key)
    let other = limiter.check(other_key);
    assert!(other.allowed, "other tenant should still be allowed");
}

// ─────────────────────────────────────────────────────────────────────────────
// Audit Log Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_audit_log_record_and_query() {
    let mut log = AuditLog::new();
    let tenant_id = Uuid::new_v4();

    log.record(tenant_id, AuditAction::TenantCreate, "tenant:test-tenant");

    let entries = log.get_for_tenant(&tenant_id);
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].action, AuditAction::TenantCreate));
}

#[test]
fn test_audit_log_get_all() {
    let mut log = AuditLog::new();
    let t1 = Uuid::new_v4();
    let t2 = Uuid::new_v4();

    log.record(t1, AuditAction::ApiKeyCreate, "key:abc");
    log.record(t2, AuditAction::TenantCreate, "tenant:t2");

    let all = log.get_all();
    assert_eq!(all.len(), 2);
}

#[test]
fn test_audit_log_query_by_action() {
    let mut log = AuditLog::new();
    let tenant_id = Uuid::new_v4();

    log.record(tenant_id, AuditAction::TenantCreate, "create");
    log.record(tenant_id, AuditAction::ApiKeyCreate, "key:1");
    log.record(tenant_id, AuditAction::ApiKeyRevoke, "key:1");

    // Filter by action (same logic as the HTTP handler)
    let mut entries: Vec<&AuditEntry> = log.get_for_tenant(&tenant_id);
    entries.retain(|e| e.action.as_str() == "api_key.create");

    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].action, AuditAction::ApiKeyCreate));
}

// ─────────────────────────────────────────────────────────────────────────────
// Enterprise HTTP Integration Tests
// ─────────────────────────────────────────────────────────────────────────────
// These tests spin up the enterprise router with in-memory state and exercise
// routes via tower::ServiceExt::oneshot (no live server needed).

#[cfg(feature = "enterprise")]
mod http_tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        Router,
    };
    use std::sync::{Arc, Mutex};
    use tower::util::ServiceExt;

    use crate::enterprise::http::{enterprise_router, EnterpriseState};

    const TEST_TOKEN: &str = "test-enterprise-token";

    /// Build a router with the enterprise routes (no auth layer — tested separately via CLI integration).
    fn test_router() -> Router {
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        enterprise_router(state)
    }

    /// POST request with auth token and JSON body.
    fn authed_post(uri: &str, body: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .header("X-Xavier-Token", TEST_TOKEN)
            .body(Body::from(body.to_string()))
            .expect("build POST request")
    }

    /// GET request with auth token.
    fn authed_get(uri: &str) -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri(uri)
            .header("X-Xavier-Token", TEST_TOKEN)
            .body(Body::empty())
            .expect("build GET request")
    }

    /// DELETE request with auth token.
    fn authed_delete(uri: &str) -> Request<Body> {
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .header("X-Xavier-Token", TEST_TOKEN)
            .body(Body::empty())
            .expect("build DELETE request")
    }

    // ── Tenant CRUD via HTTP ───────────────────────────────────────────────

    #[tokio::test]
    async fn create_tenant_returns_200_with_id() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let app = test_router();
        let resp = app
            .oneshot(authed_post(
                "/v1/tenants",
                r#"{"name":"acme","plan":"Enterprise"}"#,
            ))
            .await
            .expect("request should complete");

        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("collect body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert_eq!(json["name"], "acme");
        assert_eq!(json["plan"], "Enterprise");
        assert!(json["id"].as_str().is_some(), "id should be present");
    }

    #[tokio::test]
    async fn list_tenants_returns_created_tenant() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        let app = enterprise_router(state.clone());
        // Create tenant first
        let create_resp = app
            .clone()
            .oneshot(authed_post(
                "/v1/tenants",
                r#"{"name":"list-tenant","plan":"Pro"}"#,
            ))
            .await
            .expect("create");
        assert_eq!(create_resp.status(), StatusCode::OK);

        // Now list
        let list_resp = app.oneshot(authed_get("/v1/tenants")).await.expect("list");
        let body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("collect body");
        let tenants: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert!(
            tenants.as_array().map(|a| a.len()).unwrap_or(0) >= 1,
            "should have at least one tenant"
        );
    }

    #[tokio::test]
    async fn get_tenant_by_id_returns_correct_tenant() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        let app = enterprise_router(state.clone());
        // Create
        let create_resp = app
            .clone()
            .oneshot(authed_post(
                "/v1/tenants",
                r#"{"name":"get-by-id","plan":"Free"}"#,
            ))
            .await
            .expect("create");
        let body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let created: serde_json::Value = serde_json::from_slice(&body).expect("parse");
        let id = created["id"].as_str().expect("id field").to_string();

        // Fetch by ID
        let get_resp = app
            .oneshot(authed_get(&format!("/v1/tenants/{}", id)))
            .await
            .expect("get");
        assert_eq!(get_resp.status(), StatusCode::OK);
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let fetched: serde_json::Value = serde_json::from_slice(&get_body).expect("parse");
        assert_eq!(fetched["id"], id);
        assert_eq!(fetched["name"], "get-by-id");
    }

    #[tokio::test]
    async fn get_nonexistent_tenant_returns_404() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let app = test_router();
        let fake_id = uuid::Uuid::new_v4();
        let resp = app
            .oneshot(authed_get(&format!("/v1/tenants/{}", fake_id)))
            .await
            .expect("request should complete");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── API Key CRUD via HTTP ──────────────────────────────────────────────

    #[tokio::test]
    async fn create_api_key_returns_raw_key() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        let app = enterprise_router(state.clone());
        // Create tenant first
        let create_resp = app
            .clone()
            .oneshot(authed_post(
                "/v1/tenants",
                r#"{"name":"key-tenant","plan":"Enterprise"}"#,
            ))
            .await
            .expect("create tenant");
        let body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let tenant: serde_json::Value = serde_json::from_slice(&body).expect("parse");
        let tenant_id = tenant["id"].as_str().expect("tenant id").to_string();

        // Create API key
        let key_body = format!(
            r#"{{"tenant_id":"{}","name":"my-key","key_type":"Live"}}"#,
            tenant_id
        );
        let key_resp = app
            .oneshot(authed_post("/v1/keys", &key_body))
            .await
            .expect("create key");
        assert_eq!(key_resp.status(), StatusCode::OK);

        let key_bytes = to_bytes(key_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let key_json: serde_json::Value = serde_json::from_slice(&key_bytes).expect("parse");
        assert_eq!(key_json["name"], "my-key");
        assert_eq!(key_json["tenant_id"], tenant_id);
        // raw_key must be present (only shown once at creation)
        assert!(
            key_json["raw_key"]
                .as_str()
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            "raw_key must be non-empty"
        );
    }

    #[tokio::test]
    async fn create_api_key_for_missing_tenant_returns_404() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let app = test_router();
        let fake_id = uuid::Uuid::new_v4();
        let body = format!(
            r#"{{"tenant_id":"{}","name":"orphan-key","key_type":"Test"}}"#,
            fake_id
        );
        let resp = app
            .oneshot(authed_post("/v1/keys", &body))
            .await
            .expect("request should complete");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn revoke_api_key_returns_204() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        let app = enterprise_router(state.clone());
        // Setup: tenant + key
        let t_resp = app
            .clone()
            .oneshot(authed_post(
                "/v1/tenants",
                r#"{"name":"revoke-tenant","plan":"Pro"}"#,
            ))
            .await
            .expect("create tenant");
        let t_body = to_bytes(t_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let tenant: serde_json::Value = serde_json::from_slice(&t_body).expect("parse");
        let tenant_id = tenant["id"].as_str().expect("id").to_string();

        let k_resp = app
            .clone()
            .oneshot(authed_post(
                "/v1/keys",
                &format!(
                    r#"{{"tenant_id":"{}","name":"revoke-me","key_type":"Live"}}"#,
                    tenant_id
                ),
            ))
            .await
            .expect("create key");
        let k_body = to_bytes(k_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let key: serde_json::Value = serde_json::from_slice(&k_body).expect("parse");
        let key_id = key["id"].as_str().expect("key id").to_string();

        // Revoke
        let revoke_resp = app
            .oneshot(authed_delete(&format!("/v1/keys/{}", key_id)))
            .await
            .expect("revoke");
        assert_eq!(revoke_resp.status(), StatusCode::NO_CONTENT);
    }

    // ── Audit Log via HTTP ─────────────────────────────────────────────────

    #[tokio::test]
    async fn audit_log_records_key_creation() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        let app = enterprise_router(state.clone());
        // Create tenant
        let t_resp = app
            .clone()
            .oneshot(authed_post(
                "/v1/tenants",
                r#"{"name":"audit-tenant","plan":"Enterprise"}"#,
            ))
            .await
            .expect("create tenant");
        let t_body = to_bytes(t_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let tenant: serde_json::Value = serde_json::from_slice(&t_body).expect("parse");
        let tenant_id = tenant["id"].as_str().expect("id").to_string();

        // Create a key (should log ApiKeyCreate audit entry)
        app.clone()
            .oneshot(authed_post(
                "/v1/keys",
                &format!(
                    r#"{{"tenant_id":"{}","name":"audit-key","key_type":"Live"}}"#,
                    tenant_id
                ),
            ))
            .await
            .expect("create key");

        // Query audit log for this tenant
        let audit_resp = app
            .oneshot(authed_get(&format!("/v1/audit?tenant_id={}", tenant_id)))
            .await
            .expect("query audit");
        assert_eq!(audit_resp.status(), StatusCode::OK);

        let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let entries: serde_json::Value = serde_json::from_slice(&audit_body).expect("parse");
        let arr = entries.as_array().expect("entries should be array");
        // Should have at least the ApiKeyCreate entry
        assert!(
            arr.iter()
                .any(|e| e["action"].as_str() == Some("ApiKeyCreate")),
            "expected ApiKeyCreate entry in audit log, got: {:?}",
            arr
        );
    }

    // ── Rate Limits via HTTP ───────────────────────────────────────────────

    #[tokio::test]
    async fn get_rate_limits_returns_200() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let app = test_router();
        let resp = app
            .oneshot(authed_get("/v1/rate-limits"))
            .await
            .expect("request should complete");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn patch_rate_limits_returns_200() {
        std::env::set_var("XAVIER_TOKEN", TEST_TOKEN);
        let state = Arc::new(Mutex::new(EnterpriseState {
            tenant_store: crate::enterprise::tenancy::TenantStore::new(),
            api_key_store: crate::enterprise::keys::ApiKeyStore::new(),
            audit_log: crate::enterprise::audit::AuditLog::new(),
            rate_limiter: crate::enterprise::rate_limit::RateLimiter::new(),
            db: None,
        }));
        let app = enterprise_router(state.clone());
        let tenant_id = uuid::Uuid::new_v4();
        let body = format!(
            r#"{{"tenant_id":"{}","config":{{"rpm":120,"burst":60}}}}"#,
            tenant_id
        );
        let req = Request::builder()
            .method("PATCH")
            .uri("/v1/rate-limits")
            .header("content-type", "application/json")
            .header("X-Xavier-Token", TEST_TOKEN)
            .body(Body::from(body))
            .expect("build PATCH request");

        let resp = app.oneshot(req).await.expect("request should complete");
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
