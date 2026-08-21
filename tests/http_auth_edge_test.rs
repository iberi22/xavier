//! REST API Authentication Edge Case & 100% Branch Coverage Tests
//!
//! Tests timing-attack-resistant constant-time token comparison,
//! header extraction edge cases, token validation, and Axum auth middleware.

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    middleware::from_fn,
    response::IntoResponse,
    routing::get,
    Extension, Router,
};
use http_body_util::BodyExt;
use std::sync::Mutex;
use tower::ServiceExt;

use xavier::security::auth::{generate_jwt, Claims, User};
use xavier::server::http::api::{
    api_auth_middleware, constant_time_compare, extract_auth_token, validate_api_token,
};

// Global lock to prevent environment variable race conditions in tests.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    vars: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new(keys: &[&'static str]) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let vars = keys.iter().map(|&k| (k, std::env::var(k).ok())).collect();
        Self { _lock: lock, vars }
    }

    fn set(&self, key: &'static str, value: &str) {
        std::env::set_var(key, value);
    }

    fn remove(&self, key: &'static str) {
        std::env::remove_var(key);
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, orig_val) in &self.vars {
            if let Some(val) = orig_val {
                std::env::set_var(key, val);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

// ─── Constant-Time Comparison Tests ────────────────────────────────────────

#[test]
fn test_constant_time_compare_matching() {
    assert!(constant_time_compare(
        "exact_secret_token",
        "exact_secret_token"
    ));
    assert!(constant_time_compare("", ""));
    assert!(constant_time_compare("a", "a"));
}

#[test]
fn test_constant_time_compare_mismatched_length() {
    assert!(!constant_time_compare("short", "longer_token_value"));
    assert!(!constant_time_compare("longer_token_value", "short"));
    assert!(!constant_time_compare("", "non_empty"));
    assert!(!constant_time_compare("non_empty", ""));
}

#[test]
fn test_constant_time_compare_same_length_mismatch() {
    // Differ at start
    assert!(!constant_time_compare(
        "Asecret_token_123",
        "Bsecret_token_123"
    ));
    // Differ in middle
    assert!(!constant_time_compare(
        "secret_Xoken_123",
        "secret_Yoken_123"
    ));
    // Differ at end
    assert!(!constant_time_compare(
        "secret_token_123A",
        "secret_token_123B"
    ));
}

// ─── Extract Auth Token Header Edge Cases ───────────────────────────────────

#[test]
fn test_extract_auth_token_x_xavier_token_valid() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Xavier-Token", "my_xavier_token_123".parse().unwrap());
    let token = extract_auth_token(&headers).unwrap();
    assert_eq!(token, "my_xavier_token_123");
}

#[test]
fn test_extract_auth_token_x_xavier_token_with_spaces() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Xavier-Token", "   spaced_token_456   ".parse().unwrap());
    let token = extract_auth_token(&headers).unwrap();
    assert_eq!(token, "spaced_token_456");
}

#[test]
fn test_extract_auth_token_x_xavier_token_empty() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Xavier-Token", "".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, code, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code, "UNAUTHORIZED");
    assert_eq!(msg, "Empty authentication token");
}

#[test]
fn test_extract_auth_token_x_xavier_token_spaces_only() {
    let mut headers = HeaderMap::new();
    headers.insert("X-Xavier-Token", "     ".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(msg, "Empty authentication token");
}

#[test]
fn test_extract_auth_token_x_xavier_token_invalid_utf8() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Xavier-Token",
        HeaderValue::from_bytes(b"invalid\xFFutf8").unwrap(),
    );
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, code, msg) = err.details();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(code, "BAD_REQUEST");
    assert_eq!(msg, "Malformed X-Xavier-Token header");
}

#[test]
fn test_extract_auth_token_authorization_bearer_valid() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        "Bearer valid_bearer_token".parse().unwrap(),
    );
    let token = extract_auth_token(&headers).unwrap();
    assert_eq!(token, "valid_bearer_token");
}

#[test]
fn test_extract_auth_token_authorization_bearer_empty() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer ".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(msg, "Empty authentication token");
}

#[test]
fn test_extract_auth_token_authorization_bearer_spaces_only() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Bearer        ".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(msg, "Empty authentication token");
}

#[test]
fn test_extract_auth_token_authorization_legacy_token_valid() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "token legacy_token_xyz".parse().unwrap());
    let token = extract_auth_token(&headers).unwrap();
    assert_eq!(token, "legacy_token_xyz");
}

#[test]
fn test_extract_auth_token_authorization_legacy_token_empty() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "token   ".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(msg, "Empty authentication token");
}

#[test]
fn test_extract_auth_token_authorization_basic_unsupported() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "Basic dXNlcjpwYXNz".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, code, msg) = err.details();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(code, "BAD_REQUEST");
    assert_eq!(msg, "Unsupported authentication scheme");
}

#[test]
fn test_extract_auth_token_authorization_raw_token() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "raw_token_without_scheme".parse().unwrap());
    let token = extract_auth_token(&headers).unwrap();
    assert_eq!(token, "raw_token_without_scheme");
}

#[test]
fn test_extract_auth_token_authorization_empty() {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", "".parse().unwrap());
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(msg, "Empty authentication token");
}

#[test]
fn test_extract_auth_token_authorization_invalid_utf8() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        HeaderValue::from_bytes(b"Bearer \xFE\xFF").unwrap(),
    );
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(msg, "Malformed Authorization header");
}

#[test]
fn test_extract_auth_token_missing_headers() {
    let headers = HeaderMap::new();
    let err = extract_auth_token(&headers).unwrap_err();
    let (status, _, msg) = err.details();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(msg, "Missing Authorization header");
}

// ─── Validate API Token Tests ───────────────────────────────────────────────

#[test]
fn test_validate_api_token_unconfigured_server() {
    let err = validate_api_token("provided_token", "").unwrap_err();
    let (status, code, msg) = err.details();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(code, "INTERNAL_ERROR");
    assert_eq!(msg, "Security token not configured");
}

#[test]
fn test_validate_api_token_empty_provided() {
    assert!(!validate_api_token("", "server_token").unwrap());
    assert!(!validate_api_token("   ", "server_token").unwrap());
}

#[test]
fn test_validate_api_token_match_and_mismatch() {
    assert!(validate_api_token("server_token", "server_token").unwrap());
    assert!(validate_api_token("  server_token  ", "server_token").unwrap());
    assert!(!validate_api_token("wrong_token", "server_token").unwrap());
}

// ─── Axum Middleware End-to-End Tests ───────────────────────────────────────

async fn dummy_handler(Extension(claims): Extension<Claims>) -> impl IntoResponse {
    (StatusCode::OK, format!("Hello, {}!", claims.sub))
}

fn create_test_app() -> Router {
    Router::new()
        .route("/api/test", get(dummy_handler))
        .layer(from_fn(api_auth_middleware))
}

#[tokio::test]
async fn test_middleware_unconfigured_server_token() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.remove("XAVIER_TOKEN");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("X-Xavier-Token", "some_token")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["code"], "INTERNAL_ERROR");
    assert_eq!(json["message"], "Security token not configured");
}

#[tokio::test]
async fn test_middleware_missing_auth_header() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "error");
    assert_eq!(json["message"], "Missing Authorization header");
}

#[tokio::test]
async fn test_middleware_empty_auth_header() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "Bearer ")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"], "Empty authentication token");
}

#[tokio::test]
async fn test_middleware_unsupported_scheme() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "Basic user:pass")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"], "Unsupported authentication scheme");
}

#[tokio::test]
async fn test_middleware_valid_x_xavier_token() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("X-Xavier-Token", "valid_server_token_123")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(body_str, "Hello, root!");
}

#[tokio::test]
async fn test_middleware_valid_bearer_token() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "Bearer valid_server_token_123")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(body_str, "Hello, root!");
}

#[tokio::test]
async fn test_middleware_valid_legacy_token() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "token valid_server_token_123")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(body_str, "Hello, root!");
}

#[tokio::test]
async fn test_middleware_valid_raw_token() {
    let env = EnvGuard::new(&["XAVIER_TOKEN"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "valid_server_token_123")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(body_str, "Hello, root!");
}

#[tokio::test]
async fn test_middleware_invalid_token_no_jwt_fallback() {
    let env = EnvGuard::new(&["XAVIER_TOKEN", "XAVIER_JWT_SECRET"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");
    env.remove("XAVIER_JWT_SECRET");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "Bearer invalid_token_xyz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"], "Invalid API token");
}

#[tokio::test]
async fn test_middleware_invalid_token_jwt_fallback_success() {
    let env = EnvGuard::new(&["XAVIER_TOKEN", "XAVIER_JWT_SECRET"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");
    let jwt_secret = "super_secret_jwt_key_32bytes_long!!";
    env.set("XAVIER_JWT_SECRET", jwt_secret);

    let user = User::new(
        "user_jwt@swal.dev".to_string(),
        "JWT User".to_string(),
        xavier::security::auth::UserRole::User,
    );
    let jwt_token = generate_jwt(&user, jwt_secret.as_bytes()).unwrap();

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", format!("Bearer {jwt_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert_eq!(body_str, format!("Hello, {}!", user.id));
}

#[tokio::test]
async fn test_middleware_invalid_token_jwt_fallback_invalid_jwt() {
    let env = EnvGuard::new(&["XAVIER_TOKEN", "XAVIER_JWT_SECRET"]);
    env.set("XAVIER_TOKEN", "valid_server_token_123");
    env.set("XAVIER_JWT_SECRET", "super_secret_jwt_key");

    let app = create_test_app();
    let req = Request::builder()
        .uri("/api/test")
        .header("Authorization", "Bearer not_a_valid_jwt_string")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"], "Invalid API token");
}
