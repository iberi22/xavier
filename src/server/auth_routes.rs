//! HTTP routes for Google OAuth2 integration.
//!
//! Provides routes for connecting, status checking, OAuth callback, and disconnecting Google OAuth2 credentials.
//!
//! Handlers:
//! - GET /auth/google/status -> returns connection status and user email
//! - GET /auth/google/connect -> generates state and authorization URL
//! - GET /auth/google/callback -> code exchange, JWT payload email extraction, token storage
//! - DELETE /auth/google/disconnect -> clears stored tokens and status

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Configuration for Google OAuth2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl GoogleOAuthConfig {
    /// Loads OAuth configuration from environment variables with safe defaults for development.
    pub fn from_env() -> Self {
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID")
            .unwrap_or_else(|_| "dev-google-client-id".to_string());
        // TODO: Read client_secret from Clavis vault in production instead of environment variable.
        let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET")
            .unwrap_or_else(|_| "dev-google-client-secret".to_string());
        let redirect_uri = std::env::var("GOOGLE_OAUTH_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8006/auth/google/callback".to_string());

        Self {
            client_id,
            client_secret,
            redirect_uri,
        }
    }
}

impl Default for GoogleOAuthConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

/// State manager for Google OAuth2 connection status and tokens.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoogleOAuthManager {
    pub is_connected: bool,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub email: Option<String>,
    pub state_token: Option<String>,
    pub expires_at: Option<i64>,
}

impl GoogleOAuthManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates current connection status ("connected", "expired", or "disconnected").
    pub fn status_string(&self) -> String {
        if !self.is_connected {
            return "disconnected".to_string();
        }
        if let Some(exp) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if now >= exp {
                return "expired".to_string();
            }
        }
        "connected".to_string()
    }

    /// Clears active session and resets state to disconnected.
    pub fn disconnect(&mut self) {
        self.is_connected = false;
        self.access_token = None;
        self.refresh_token = None;
        self.id_token = None;
        self.email = None;
        self.state_token = None;
        self.expires_at = None;
    }
}

/// Shared state container for Axum router.
#[derive(Clone)]
pub struct GoogleOAuthState {
    pub config: GoogleOAuthConfig,
    pub manager: Arc<Mutex<GoogleOAuthManager>>,
}

impl GoogleOAuthState {
    pub fn new(config: GoogleOAuthConfig) -> Self {
        Self {
            config,
            manager: Arc::new(Mutex::new(GoogleOAuthManager::new())),
        }
    }
}

impl Default for GoogleOAuthState {
    fn default() -> Self {
        Self::new(GoogleOAuthConfig::default())
    }
}

/// Response payload for GET /auth/google/status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleAuthStatusResponse {
    pub status: String,
    pub connected: bool,
    pub email: Option<String>,
}

/// Response payload for GET /auth/google/connect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleAuthConnectResponse {
    pub auth_url: String,
    pub state: String,
}

/// Query parameters received by GET /auth/google/callback.
#[derive(Debug, Clone, Deserialize)]
pub struct GoogleOAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Helper function to URL-encode form key-value pairs.
fn form_urlencode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", pct_encode(k), pct_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Decodes base64url encoded string.
fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut s = s.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 {
        s.push('=');
    }
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Some(out)
}

/// Extracts email field from raw JWT `id_token` payload.
pub fn extract_email_from_jwt(id_token: &str) -> Option<String> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let decoded = b64url_decode(parts[1])?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    json.get("email").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// GET /auth/google/status
/// Returns connection status, connection boolean, and connected user email.
pub async fn status_handler(
    State(state): State<GoogleOAuthState>,
) -> impl IntoResponse {
    let mgr = state.manager.lock().unwrap();
    let status = mgr.status_string();
    let connected = status == "connected";
    let email = mgr.email.clone();

    (
        StatusCode::OK,
        Json(GoogleAuthStatusResponse {
            status,
            connected,
            email,
        }),
    )
}

/// GET /auth/google/connect
/// Initiates Google OAuth2 flow by generating state token and returning authorization URL.
pub async fn connect_handler(
    State(state): State<GoogleOAuthState>,
) -> impl IntoResponse {
    let state_token = format!("{:x}", rand::random::<u128>());
    {
        let mut mgr = state.manager.lock().unwrap();
        mgr.state_token = Some(state_token.clone());
    }

    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
        pct_encode(&state.config.client_id),
        pct_encode(&state.config.redirect_uri),
        pct_encode("openid email profile"),
        pct_encode(&state_token),
    );

    (
        StatusCode::OK,
        Json(GoogleAuthConnectResponse {
            auth_url,
            state: state_token,
        }),
    )
}

/// GET /auth/google/callback
/// Exchanges authorization code for tokens, extracts email from JWT id_token, and stores state.
pub async fn callback_handler(
    State(state): State<GoogleOAuthState>,
    Query(query): Query<GoogleOAuthCallbackQuery>,
) -> impl IntoResponse {
    if let Some(err) = query.error {
        let desc = query.error_description.unwrap_or_default();
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "oauth_callback_error",
                "details": err,
                "description": desc,
            })),
        )
            .into_response();
    }

    // Validate state token to prevent CSRF
    let expected_state = {
        let mut mgr = state.manager.lock().unwrap();
        mgr.state_token.take()
    };

    match (query.state.as_deref(), expected_state.as_deref()) {
        (Some(received), Some(expected)) if !received.is_empty() && received == expected => {}
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "invalid_state",
                    "details": "OAuth state mismatch or missing state token"
                })),
            )
                .into_response();
        }
    }

    let code = match query.code {
        Some(c) if !c.is_empty() => c,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "missing_code" })),
            )
                .into_response();
        }
    };

    // TODO: Read client_secret from Clavis in production instead of environment variable.
    let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET")
        .unwrap_or_else(|_| state.config.client_secret.clone());

    let body = form_urlencode(&[
        ("code", code.as_str()),
        ("client_id", state.config.client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("redirect_uri", state.config.redirect_uri.as_str()),
        ("grant_type", "authorization_code"),
    ]);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let resp = match client
        .post("https://oauth2.googleapis.com/token")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "token_exchange_network_error",
                    "details": e.to_string(),
                })),
            )
                .into_response();
        }
    };

    let status = resp.status();
    let token_json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);

    if !status.is_success() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "token_exchange_failed",
                "status": status.as_u16(),
                "response": token_json,
            })),
        )
            .into_response();
    }

    let access_token = token_json.get("access_token").and_then(|v| v.as_str()).map(|s| s.to_string());
    let refresh_token = token_json.get("refresh_token").and_then(|v| v.as_str()).map(|s| s.to_string());
    let id_token = token_json.get("id_token").and_then(|v| v.as_str()).map(|s| s.to_string());
    let expires_in = token_json.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(3600);

    let email = id_token.as_deref().and_then(extract_email_from_jwt);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    {
        let mut mgr = state.manager.lock().unwrap();
        mgr.is_connected = true;
        mgr.access_token = access_token;
        mgr.refresh_token = refresh_token;
        mgr.id_token = id_token;
        mgr.email = email;
        mgr.expires_at = Some(now + expires_in);
    }

    Redirect::to("/panel").into_response()
}

/// DELETE /auth/google/disconnect
/// Clears Google OAuth2 token state and returns 200 OK.
pub async fn disconnect_handler(
    State(state): State<GoogleOAuthState>,
) -> impl IntoResponse {
    let mut mgr = state.manager.lock().unwrap();
    mgr.disconnect();

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "disconnected"
        })),
    )
}

/// Constructs the Axum Router for Google OAuth2 endpoints.
pub fn router(state: GoogleOAuthState) -> Router {
    Router::new()
        .route("/status", get(status_handler))
        .route("/connect", get(connect_handler))
        .route("/callback", get(callback_handler))
        .route("/disconnect", delete(disconnect_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_email_from_jwt() {
        // JWT header: {"alg":"HS256","typ":"JWT"} -> eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9
        // JWT payload: {"sub":"123","email":"test@example.com"} -> eyJzdWIiOiIxMjMiLCJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20ifQ
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjMiLCJlbWFpbCI6InRlc3RAZXhhbXBsZS5jb20ifQ.signature";
        let email = extract_email_from_jwt(jwt);
        assert_eq!(email, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_google_oauth_manager_disconnect() {
        let mut mgr = GoogleOAuthManager::new();
        mgr.is_connected = true;
        mgr.email = Some("user@gmail.com".to_string());
        mgr.access_token = Some("token123".to_string());
        assert_eq!(mgr.status_string(), "connected");

        mgr.disconnect();
        assert_eq!(mgr.status_string(), "disconnected");
        assert!(!mgr.is_connected);
        assert!(mgr.email.is_none());
        assert!(mgr.access_token.is_none());
    }

    #[tokio::test]
    async fn test_router_construction_and_handlers() {
        let state = GoogleOAuthState::default();
        let app = router(state.clone());

        // Test GET /status
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/status")
            .body(axum::body::Body::empty())
            .unwrap();

        use tower::ServiceExt;
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Test GET /connect
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/connect")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Test GET /callback with invalid state
        let req = axum::http::Request::builder()
            .method("GET")
            .uri("/callback?code=testcode&state=invalidstate")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        // Test DELETE /disconnect
        let req = axum::http::Request::builder()
            .method("DELETE")
            .uri("/disconnect")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
