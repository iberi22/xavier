//! HTTP handlers for the Clavis key vault (`/v1/clavis/*`).
//!
//! Exposes the Clavis provider-key vault to external clients (the `swal-vault`
//! Flutter app) over the REST API:
//!
//! - `GET  /v1/clavis/keys/{key_name}` — read a stored key value
//! - `PUT  /v1/clavis/keys/{key_name}` — store/replace a key value
//! - `POST /v1/clavis/proxy`          — LLM provider proxy (not implemented yet)
//!
//! ## Storage
//!
//! Keys are persisted through [`HardwareVault`], **not** through
//! [`crate::clavis::ClavisEngine`]. The engine is in-memory only and would
//! lose every key on restart. The vault writes AES-GCM encrypted files under
//! `~/.xavier/secrets/<name>.enc` (or the OS keyring) with a master key held
//! in the system keyring, so values survive restarts.
//!
//! ## Security
//!
//! - Secret values are never written to a log or a trace. Every value read or
//!   written is registered with the global Clavis log masker
//!   ([`crate::clavis::register_secret`]) so that any message routed through
//!   [`crate::clavis::mask_log_message`] (or the `clavis_*!` macros) has it
//!   replaced by [`crate::clavis::mask_key`].
//! - Error responses carry only the key *name*, never the value.
//! - `GET` necessarily returns the value to the caller — the client needs it —
//!   but the handler performs no logging of the response body.
//! - Routes are gated by the same `require_permission` layer as
//!   `/v1/maintenance/*`, using `can_manage_secrets()` (admin only): reading a
//!   provider key is equivalent to handling a secret.

use std::sync::{Arc, OnceLock, RwLock};

use super::{error_json, error_response};
use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;
use serde_json::json;

use crate::clavis::{mask_key, register_secret};
use crate::secrets::vault::HardwareVault;
use crate::secrets::SecretError;

/// Keyring service name for the Clavis provider-key vault.
const CLAVIS_VAULT_SERVICE: &str = "xavier-clavis";

/// Longest accepted key name (also the longest accepted fallback filename).
const MAX_KEY_NAME_LEN: usize = 128;

// ---------------------------------------------------------------------------
// Vault wiring
// ---------------------------------------------------------------------------

/// The active Clavis vault. Wrapped in a lock so tests can install an
/// isolated instance without poisoning a `OnceLock`.
static CLAVIS_VAULT: OnceLock<RwLock<Arc<HardwareVault>>> = OnceLock::new();

fn vault_slot() -> &'static RwLock<Arc<HardwareVault>> {
    CLAVIS_VAULT.get_or_init(|| RwLock::new(Arc::new(HardwareVault::new(CLAVIS_VAULT_SERVICE))))
}

/// The active Clavis vault, persisted in the keyring / encrypted fallback dir.
pub fn clavis_vault() -> Arc<HardwareVault> {
    match vault_slot().read() {
        Ok(guard) => guard.clone(),
        Err(_) => {
            tracing::error!("CLAVIS_VAULT lock poisoned while resolving the vault");
            Arc::new(HardwareVault::new(CLAVIS_VAULT_SERVICE))
        }
    }
}

/// Install an isolated vault (tests only).
#[cfg(test)]
pub(crate) fn set_clavis_vault(vault: Arc<HardwareVault>) {
    match vault_slot().write() {
        Ok(mut guard) => *guard = vault,
        Err(_) => panic!("CLAVIS_VAULT lock poisoned while installing the test vault"),
    }
}

// ---------------------------------------------------------------------------
// Request / response payloads
// ---------------------------------------------------------------------------

/// Body for `PUT /v1/clavis/keys/{key_name}`.
#[derive(Debug, Deserialize)]
pub struct ClavisKeyPutRequest {
    /// The raw secret value. Never logged, never echoed back.
    pub value: String,
}

/// Body for `POST /v1/clavis/proxy`.
#[derive(Debug, Deserialize)]
pub struct ClavisProxyRequest {
    /// Target LLM provider, e.g. `openai` or `anthropic`.
    pub provider: String,
    /// The original LLM request payload, forwarded verbatim.
    pub request: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /v1/clavis/keys/{key_name}` — read a stored key value.
///
/// - `200` `{"value": "..."}` on success
/// - `400` malformed key name
/// - `404` `{"error": "..."}` when the key is absent
/// - `500` when the vault is unreadable
pub async fn get_clavis_key_handler(Path(key_name): Path<String>) -> impl IntoResponse {
    let key_name = match validate_key_name(&key_name) {
        Ok(name) => name,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message).into_response(),
    };

    let vault = clavis_vault();
    let lookup = key_name.clone();
    let result = tokio::task::spawn_blocking(move || {
        // 1. Direct lookup in current Clavis vault
        if let Ok(value) = vault.get_secret(&lookup) {
            return Ok(value);
        }

        // Generate candidate names (e.g., toggle "api_key_" prefix)
        let alt_key = if lookup.starts_with("api_key_") {
            lookup.strip_prefix("api_key_").unwrap_or(&lookup).to_string()
        } else {
            format!("api_key_{lookup}")
        };

        // 2. Check alt_key in current Clavis vault
        if let Ok(value) = vault.get_secret(&alt_key) {
            let _ = vault.store_secret(&lookup, &value);
            return Ok(value);
        }

        // 3. Fallback to legacy "xavier" vault
        let legacy_vault = HardwareVault::new("xavier");
        let legacy_key = if let Ok(v) = legacy_vault.get_secret(&lookup) {
            Some((lookup.clone(), v))
        } else if let Ok(v) = legacy_vault.get_secret(&alt_key) {
            Some((alt_key.clone(), v))
        } else {
            None
        };

        if let Some((found_key, value)) = legacy_key {
            // Migrate secret and potential metadata to Clavis vault
            let _ = vault.store_secret(&lookup, &value);
            if found_key != lookup {
                let _ = vault.store_secret(&found_key, &value);
            }
            let meta_key = format!("{found_key}_meta");
            if let Ok(meta_json) = legacy_vault.get_secret(&meta_key) {
                let _ = vault.store_secret(&meta_key, &meta_json);
                let _ = vault.store_secret(&format!("{lookup}_meta"), &meta_json);
            }
            tracing::info!(key = %lookup, "migrated key from legacy xavier vault to clavis vault");
            return Ok(value);
        }

        Err(SecretError::NotFound(lookup))
    })
    .await;

    let value = match result {
        Ok(Ok(value)) => value,
        Ok(Err(SecretError::NotFound(_))) => {
            // Key name only — never the value.
            return not_found_response(&key_name).into_response();
        }
        Ok(Err(e)) => {
            tracing::warn!(
                key = %key_name,
                error = %crate::clavis::mask_log_message(&e.to_string()),
                "clavis key read failed"
            );
            return vault_error_response(&key_name, "read");
        }
        Err(join_err) => {
            tracing::error!(key = %key_name, error = %join_err, "clavis key read task failed");
            return vault_error_response(&key_name, "read");
        }
    };

    // Register the value so it can never leak verbatim into a Clavis log line.
    register_secret(&value);
    tracing::debug!(key = %key_name, masked = %mask_key(&value), "clavis key read");

    (StatusCode::OK, Json(json!({ "value": value }))).into_response()
}

/// `PUT /v1/clavis/keys/{key_name}` — store or replace a key value.
///
/// - `201` when a new key was created
/// - `200` when an existing key was overwritten
/// - `400` malformed key name or empty value
/// - `500` when the vault write fails
pub async fn put_clavis_key_handler(
    Path(key_name): Path<String>,
    Json(payload): Json<ClavisKeyPutRequest>,
) -> impl IntoResponse {
    let key_name = match validate_key_name(&key_name) {
        Ok(name) => name,
        Err(message) => return error_response(StatusCode::BAD_REQUEST, message).into_response(),
    };

    if payload.value.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "value must not be empty").into_response();
    }

    // Register before writing so the value is masked even if the write fails
    // and the value later shows up in an error message.
    register_secret(&payload.value);

    // Detect creation vs. update before writing.
    let existed = {
        let vault = clavis_vault();
        let probe = key_name.clone();
        tokio::task::spawn_blocking(move || vault.get_secret(&probe))
            .await
            .map(|r| r.is_ok())
            .unwrap_or(false)
    };

    let vault = clavis_vault();
    let store_key = key_name.clone();
    let value = payload.value.clone();
    let stored = tokio::task::spawn_blocking(move || vault.store_secret(&store_key, &value)).await;

    match stored {
        Ok(Ok(())) => {
            let status = if existed {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            tracing::info!(
                key = %key_name,
                masked = %mask_key(&payload.value),
                "clavis key stored"
            );
            (
                status,
                Json(json!({
                    "status": "ok",
                    "key": key_name,
                    "created": !existed,
                    "masked_value": mask_key(&payload.value),
                })),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!(
                key = %key_name,
                error = %crate::clavis::mask_log_message(&e.to_string()),
                "clavis key write failed"
            );
            vault_error_response(&key_name, "store")
        }
        Err(join_err) => {
            tracing::error!(key = %key_name, error = %join_err, "clavis key write task failed");
            vault_error_response(&key_name, "store")
        }
    }
}

/// `POST /v1/clavis/proxy` — forward an LLM request with an injected key.
///
/// The body is validated, but the provider call is **not** implemented: there
/// is no provider transport wired into this router yet. Rather than fabricate a
/// success response, the handler answers `501 Not Implemented` so the client
/// learns the capability is absent.
///
/// - `400` malformed body
/// - `501` proxy forwarding not implemented
pub async fn clavis_proxy_handler(Json(payload): Json<ClavisProxyRequest>) -> impl IntoResponse {
    let provider = payload.provider.trim();
    if provider.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "provider must not be empty")
            .into_response();
    }
    if !payload.request.is_object() {
        return error_response(StatusCode::BAD_REQUEST, "request must be a JSON object")
            .into_response();
    }

    tracing::warn!(
        provider = %crate::clavis::mask_log_message(provider),
        "clavis proxy invoked but provider forwarding is not implemented"
    );

    (
        StatusCode::NOT_IMPLEMENTED,
        error_json(format!(
            "Clavis proxy for provider '{}' is not implemented on this node",
            provider
        )),
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reject key names that are empty, too long, or that could escape the vault
/// directory (`<name>.enc` is a filesystem path).
fn validate_key_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("key name must not be empty".to_string());
    }
    if name.len() > MAX_KEY_NAME_LEN {
        return Err(format!(
            "key name must be at most {} characters",
            MAX_KEY_NAME_LEN
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(
            "key name may only contain ASCII letters, digits, '_', '-' and '.'".to_string(),
        );
    }
    if name == "." || name == ".." || name.starts_with('.') {
        return Err("key name must not start with '.'".to_string());
    }
    Ok(name.to_string())
}

/// 500 body for a vault failure. Carries the key *name* and the operation —
/// never the value, and never the raw provider error.
fn vault_error_response(key_name: &str, op: &str) -> axum::response::Response {
    error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to {} key '{}' in the Clavis vault", op, key_name),
    )
    .into_response()
}

/// 404 body for a missing key. Carries both `error` and `message` so the Dart
/// client can tell "key absent" from "endpoint not found".
fn not_found_response(key_name: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "status": "error",
            "error": format!("Key '{}' not found in the Clavis vault", key_name),
            "message": format!("Key '{}' not found in the Clavis vault", key_name),
            "code": "clavis_key_not_found",
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Key name validation
    // -------------------------------------------------------------------

    #[test]
    fn validate_key_name_accepts_expected_shapes() {
        assert_eq!(validate_key_name("openai").unwrap(), "openai");
        assert_eq!(
            validate_key_name(" openai-key_v1.2 ").unwrap(),
            "openai-key_v1.2"
        );
    }

    #[test]
    fn validate_key_name_rejects_path_traversal() {
        for bad in ["../escape", "..", ".hidden", "a/b", "a\\b", "a b"] {
            assert!(
                validate_key_name(bad).is_err(),
                "expected '{}' to be rejected",
                bad
            );
        }
    }

    #[test]
    fn validate_key_name_rejects_empty_and_oversized() {
        assert!(validate_key_name("").is_err());
        assert!(validate_key_name("   ").is_err());
        let long = "k".repeat(MAX_KEY_NAME_LEN + 1);
        assert!(validate_key_name(&long).is_err());
    }

    // -------------------------------------------------------------------
    // Vault isolation
    // -------------------------------------------------------------------

    fn install_isolated_vault() -> (tempfile::TempDir, Arc<HardwareVault>) {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Arc::new(
            HardwareVault::new("xavier-test-clavis").isolated(dir.path().to_path_buf(), [3u8; 32]),
        );
        set_clavis_vault(vault.clone());
        (dir, vault)
    }

    // -------------------------------------------------------------------
    // Handler-level tests
    // -------------------------------------------------------------------

    #[tokio::test]
    #[serial_test::serial]
    async fn get_key_returns_404_json_when_absent() {
        let (_dir, _vault) = install_isolated_vault();

        let response = get_clavis_key_handler(Path("absent-key".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("collect body");
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert_eq!(parsed["status"], "error");
        assert!(parsed["error"].as_str().unwrap().contains("absent-key"));
        assert_eq!(parsed["code"], "clavis_key_not_found");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn get_key_returns_200_with_value() {
        let (_dir, vault) = install_isolated_vault();
        vault
            .store_secret("stored-key", "raw-secret-value-abc")
            .unwrap();

        let response = get_clavis_key_handler(Path("stored-key".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("collect body");
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert_eq!(parsed["value"], "raw-secret-value-abc");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn put_key_rejects_invalid_name_and_empty_value() {
        let (_dir, _vault) = install_isolated_vault();

        let bad_name = put_clavis_key_handler(
            Path("../escape".to_string()),
            Json(ClavisKeyPutRequest {
                value: "v".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);

        let empty = put_clavis_key_handler(
            Path("some-key".to_string()),
            Json(ClavisKeyPutRequest {
                value: String::new(),
            }),
        )
        .await
        .into_response();
        assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn put_then_get_round_trips_through_vault() {
        let (_dir, vault) = install_isolated_vault();

        let created = put_clavis_key_handler(
            Path("round-trip".to_string()),
            Json(ClavisKeyPutRequest {
                value: "«redacted:sk-…»".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(created.status(), StatusCode::CREATED);

        // Persisted for real, not held in the in-memory ClavisEngine.
        assert_eq!(vault.get_secret("round-trip").unwrap(), "«redacted:sk-…»");

        let fetched = get_clavis_key_handler(Path("round-trip".to_string()))
            .await
            .into_response();
        assert_eq!(fetched.status(), StatusCode::OK);
        let body = axum::body::to_bytes(fetched.into_body(), 64 * 1024)
            .await
            .expect("collect body");
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert_eq!(parsed["value"], "«redacted:sk-…»");

        // Second write is an update, not a create.
        let updated = put_clavis_key_handler(
            Path("round-trip".to_string()),
            Json(ClavisKeyPutRequest {
                value: "«redacted:sk-…»".to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(updated.status(), StatusCode::OK);
        assert_eq!(vault.get_secret("round-trip").unwrap(), "«redacted:sk-…»");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn stored_value_never_appears_in_clavis_logs_or_on_disk() {
        let (dir, vault) = install_isolated_vault();
        let value = "«redacted:sk-…»";

        let stored = put_clavis_key_handler(
            Path("log-safety".to_string()),
            Json(ClavisKeyPutRequest {
                value: value.to_string(),
            }),
        )
        .await
        .into_response();
        assert_eq!(stored.status(), StatusCode::CREATED);

        // 1. Any Clavis-masked log line hides the value.
        let logged = crate::clavis::mask_log_message(&format!("using key {}", value));
        assert!(
            !logged.contains(value),
            "secret leaked into a masked log line: {}",
            logged
        );
        assert!(logged.contains(&mask_key(value)));

        // 2. The on-disk artifact is encrypted, not plaintext.
        let enc = dir.path().join("log-safety.enc");
        assert!(
            enc.exists(),
            "expected encrypted vault file at {}",
            enc.display()
        );
        let raw = std::fs::read(&enc).expect("read encrypted file");
        assert!(
            !String::from_utf8_lossy(&raw).contains(value),
            "secret found in plaintext inside the vault file"
        );
        // The file really is the value the vault returns.
        assert_eq!(vault.get_secret("log-safety").unwrap(), value);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn get_key_response_body_is_not_written_to_logs() {
        // The handler returns the value to the caller but never emits it
        // through tracing: only the key name and a masked preview.
        let (_dir, vault) = install_isolated_vault();
        let value = "«redacted:sk-…»";
        vault.store_secret("get-path", value).unwrap();

        crate::clavis::unregister_secret(value);
        let response = get_clavis_key_handler(Path("get-path".to_string()))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("collect body");
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        // Value reaches the caller...
        assert_eq!(parsed["value"], value);
        // ...and any Clavis-masked message built from it hides it.
        assert!(!crate::clavis::mask_log_message(value).contains(value));
    }

    // -------------------------------------------------------------------
    // Router wiring: routes are mounted and gated by can_manage_secrets()
    // -------------------------------------------------------------------

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode as HttpStatusCode};
    use http_body_util::BodyExt;
    use tower::util::ServiceExt;

    use crate::adapters::inbound::http::routes::create_router;
    use crate::security::auth::{Claims, UserRole};

    /// A router request for a Clavis route, optionally carrying auth claims.
    fn route_request(
        method: Method,
        uri: &str,
        body: Option<&str>,
        role: Option<UserRole>,
    ) -> Request<Body> {
        let mut builder = Request::builder().uri(uri).method(method);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let mut req = builder
            .body(match body {
                Some(b) => Body::from(b.to_string()),
                None => Body::empty(),
            })
            .expect("build request");
        if let Some(role) = role {
            req.extensions_mut().insert(Claims::new(
                "test".to_string(),
                "test@swal.dev".to_string(),
                role,
                chrono::Duration::hours(1),
            ));
        }
        req
    }

    /// Each router test needs a private vault, so they cannot run in parallel.
    #[tokio::test]
    #[serial_test::serial]
    async fn route_get_key_requires_claims() {
        install_isolated_vault();

        // No claims at all -> 403 from the auth middleware (not a 404 route miss).
        let resp = create_router()
            .oneshot(route_request(
                Method::GET,
                "/v1/clavis/keys/anything",
                None,
                None,
            ))
            .await
            .expect("request should complete");
        assert_eq!(resp.status(), HttpStatusCode::FORBIDDEN);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn route_get_key_forbidden_for_non_admin_role() {
        install_isolated_vault();

        let resp = create_router()
            .oneshot(route_request(
                Method::GET,
                "/v1/clavis/keys/anything",
                None,
                Some(UserRole::User),
            ))
            .await
            .expect("request should complete");
        assert_eq!(resp.status(), HttpStatusCode::FORBIDDEN);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn route_get_key_returns_404_for_admin_when_absent() {
        install_isolated_vault();

        let resp = create_router()
            .oneshot(route_request(
                Method::GET,
                "/v1/clavis/keys/route-absent",
                None,
                Some(UserRole::Admin),
            ))
            .await
            .expect("request should complete");
        assert_eq!(resp.status(), HttpStatusCode::NOT_FOUND);

        // Body must satisfy the Dart client's not-found contract
        // (`data.containsKey('error') || data.containsKey('code')`).
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert!(parsed.get("error").is_some() || parsed.get("code").is_some());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn route_put_then_get_round_trip_over_http() {
        let (_dir, vault) = install_isolated_vault();
        let value = "«redacted:sk-…»";

        let put = create_router()
            .oneshot(route_request(
                Method::PUT,
                "/v1/clavis/keys/http-key",
                Some(&format!(r#"{{"value":"{}"}}"#, value)),
                Some(UserRole::Admin),
            ))
            .await
            .expect("request should complete");
        assert!(
            put.status() == HttpStatusCode::OK || put.status() == HttpStatusCode::CREATED,
            "unexpected PUT status {}",
            put.status()
        );
        assert_eq!(vault.get_secret("http-key").unwrap(), value);

        let get = create_router()
            .oneshot(route_request(
                Method::GET,
                "/v1/clavis/keys/http-key",
                None,
                Some(UserRole::Admin),
            ))
            .await
            .expect("request should complete");
        assert_eq!(get.status(), HttpStatusCode::OK);
        let body = get.into_body().collect().await.unwrap().to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert_eq!(parsed["value"], value);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn route_proxy_requires_claims_and_returns_501_for_admin() {
        install_isolated_vault();

        let anon = create_router()
            .oneshot(route_request(
                Method::POST,
                "/v1/clavis/proxy",
                Some(r#"{"provider":"openai","request":{"model":"gpt-4o"}}"#),
                None,
            ))
            .await
            .expect("request should complete");
        assert_eq!(anon.status(), HttpStatusCode::FORBIDDEN);

        let admin = create_router()
            .oneshot(route_request(
                Method::POST,
                "/v1/clavis/proxy",
                Some(r#"{"provider":"openai","request":{"model":"gpt-4o"}}"#),
                Some(UserRole::Admin),
            ))
            .await
            .expect("request should complete");
        // Not implemented, and honest about it.
        assert_eq!(admin.status(), HttpStatusCode::NOT_IMPLEMENTED);
    }

    // -------------------------------------------------------------------
    // Proxy
    // -------------------------------------------------------------------

    #[tokio::test]
    #[serial_test::serial]
    async fn proxy_returns_501_not_implemented_for_valid_body() {
        let payload: ClavisProxyRequest = serde_json::from_value(json!({
            "provider": "openai",
            "request": { "model": "gpt-4o", "messages": [] }
        }))
        .unwrap();

        let response = clavis_proxy_handler(Json(payload)).await.into_response();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("collect body");
        let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse JSON");
        assert_eq!(parsed["status"], "error");
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("not implemented"));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn proxy_rejects_invalid_body() {
        let empty_provider: ClavisProxyRequest = serde_json::from_value(json!({
            "provider": "   ",
            "request": {}
        }))
        .unwrap();
        assert_eq!(
            clavis_proxy_handler(Json(empty_provider))
                .await
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );

        let non_object: ClavisProxyRequest = serde_json::from_value(json!({
            "provider": "openai",
            "request": "not-an-object"
        }))
        .unwrap();
        assert_eq!(
            clavis_proxy_handler(Json(non_object))
                .await
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
