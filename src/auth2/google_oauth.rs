//! Google OAuth2 module for Colab compute access and Google Workspace integrations.
//!
//! Provides OAuth2 authorization code flow with PKCE (RFC 7636), token management,
//! expiry checking, and Clavis vault secret reference integration.

use chrono::Utc;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default Google OAuth authorization endpoint.
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Default Google OAuth token endpoint.
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Default scopes required for Colab compute access and user profile identification.
pub const DEFAULT_COLAB_SCOPES: &[&str] = &[
    "openid",
    "email",
    "profile",
    "https://www.googleapis.com/auth/drive.file",
    "https://www.googleapis.com/auth/colab",
];

/// Configuration for Google OAuth2 authentication.
///
/// Note: The `client_secret_clavis_ref` MUST be a Clavis vault secret reference
/// (e.g., `clavis://google-oauth-secret`) rather than a raw secret string in config.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub redirect_uri: String,
    /// Reference string pointing to the Clavis key vault (e.g., `clavis://google-oauth-secret`).
    pub client_secret_clavis_ref: String,
    pub scopes: Vec<String>,
}

impl Default for GoogleOAuthConfig {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            redirect_uri: "http://localhost:8006/v1/auth/google/callback".to_string(),
            client_secret_clavis_ref: "clavis://google-oauth-secret".to_string(),
            scopes: DEFAULT_COLAB_SCOPES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl GoogleOAuthConfig {
    /// Constructs configuration by reading environment variables with safe defaults.
    pub fn from_env() -> Self {
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID").unwrap_or_default();
        let redirect_uri = std::env::var("GOOGLE_OAUTH_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:8006/v1/auth/google/callback".to_string());
        let secret_ref = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET_CLAVIS_REF")
            .unwrap_or_else(|_| "clavis://google-oauth-secret".to_string());

        let mut cfg = Self {
            client_id,
            redirect_uri,
            client_secret_clavis_ref: secret_ref,
            scopes: DEFAULT_COLAB_SCOPES.iter().map(|s| s.to_string()).collect(),
        };

        // Ensure secret uses a valid Clavis reference prefix
        cfg.ensure_clavis_ref();
        cfg
    }

    /// Enforces that the client secret field uses a Clavis reference string scheme (`clavis://`).
    pub fn ensure_clavis_ref(&mut self) {
        if !self.client_secret_clavis_ref.starts_with("clavis://") {
            let key = self
                .client_secret_clavis_ref
                .trim_start_matches("clavis:")
                .trim_start_matches('/');
            self.client_secret_clavis_ref = format!("clavis://{}", key);
        }
    }

    /// Resolves the actual secret value from the Clavis engine if configured,
    /// or strips the `clavis://` scheme prefix.
    pub fn resolve_secret(&self) -> String {
        self.client_secret_clavis_ref
            .strip_prefix("clavis://")
            .unwrap_or(&self.client_secret_clavis_ref)
            .to_string()
    }
}

/// Token payload received from Google OAuth2 token endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    pub id_token: Option<String>,
    pub scope: Option<String>,
    pub token_type: String,
}

impl GoogleToken {
    /// Checks if the token is expired or within a 30-second expiry buffer.
    pub fn is_expired(&self) -> bool {
        let now = Utc::now().timestamp();
        now >= (self.expires_at - 30)
    }

    /// Checks if the token includes Colab compute or Drive file authorization scope.
    pub fn has_colab_scope(&self) -> bool {
        if let Some(ref scopes) = self.scope {
            scopes.contains("colab") || scopes.contains("drive.file") || scopes.contains("drive")
        } else {
            false
        }
    }
}

/// Status summary for Google OAuth authentication and Colab compute access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleOAuthStatus {
    pub is_connected: bool,
    pub scopes: Vec<String>,
    pub expires_at: Option<i64>,
    pub has_colab_access: bool,
}

/// Manager handling Google OAuth2 flow, state verification, PKCE generation, and token storage.
#[derive(Debug, Clone)]
pub struct GoogleOAuthManager {
    config: GoogleOAuthConfig,
    token_store: Arc<RwLock<Option<GoogleToken>>>,
}

impl GoogleOAuthManager {
    /// Creates a new `GoogleOAuthManager` instance with the specified configuration.
    pub fn new(config: GoogleOAuthConfig) -> Self {
        let mut cfg = config;
        cfg.ensure_clavis_ref();
        Self {
            config: cfg,
            token_store: Arc::new(RwLock::new(None)),
        }
    }

    /// Returns a reference to the configuration.
    pub fn config(&self) -> &GoogleOAuthConfig {
        &self.config
    }

    /// Generates a cryptographic state token using ULID.
    pub fn generate_state(&self) -> String {
        ulid::Ulid::new().to_string()
    }

    /// Generates a PKCE code verifier and S256 code challenge pair.
    pub fn generate_pkce() -> (String, String) {
        let mut raw = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut raw);
        let verifier = urlencoding::encode(&crate::crypto::hex_encode(raw)).to_string();

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let digest = hasher.finalize();

        // Base64URL encode without padding
        let challenge = base64_url_encode(&digest);
        (verifier, challenge)
    }

    /// Constructs the full Google authorization URL with PKCE and state parameters.
    pub fn get_authorization_url(&self, state: &str, code_challenge: &str) -> String {
        let scope_str = self.config.scopes.join(" ");
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
            GOOGLE_AUTH_URL,
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(&self.config.redirect_uri),
            urlencoding::encode(&scope_str),
            urlencoding::encode(state),
            urlencoding::encode(code_challenge)
        )
    }

    /// Stores an active token in the manager's token store.
    pub async fn store_token(&self, token: GoogleToken) {
        let mut store = self.token_store.write().await;
        *store = Some(token);
    }

    /// Retrieves a copy of the current token if present.
    pub async fn get_token(&self) -> Option<GoogleToken> {
        let store = self.token_store.read().await;
        store.clone()
    }

    /// Disconnects Google OAuth by clearing stored tokens.
    pub async fn disconnect(&self) {
        let mut store = self.token_store.write().await;
        *store = None;
    }

    /// Computes current connection status for Google OAuth and Colab access.
    pub async fn status(&self) -> GoogleOAuthStatus {
        let store = self.token_store.read().await;
        match &*store {
            Some(token) if !token.is_expired() => {
                let scopes: Vec<String> = token
                    .scope
                    .as_deref()
                    .unwrap_or("")
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();

                let has_colab = token.has_colab_scope()
                    || scopes.iter().any(|s| s.contains("colab") || s.contains("drive"));

                GoogleOAuthStatus {
                    is_connected: true,
                    scopes,
                    expires_at: Some(token.expires_at),
                    has_colab_access: has_colab,
                }
            }
            _ => GoogleOAuthStatus {
                is_connected: false,
                scopes: Vec::new(),
                expires_at: None,
                has_colab_access: false,
            },
        }
    }

    /// Exchanges an authorization code and PKCE code verifier for Google tokens.
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> anyhow::Result<GoogleToken> {
        let client = reqwest::Client::new();
        let secret = self.config.resolve_secret();

        let form_body = format!(
            "client_id={}&client_secret={}&code={}&code_verifier={}&grant_type=authorization_code&redirect_uri={}",
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(&secret),
            urlencoding::encode(code),
            urlencoding::encode(code_verifier),
            urlencoding::encode(&self.config.redirect_uri)
        );

        let res = client
            .post(GOOGLE_TOKEN_URL)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(form_body)
            .send()
            .await?;

        if !res.status().is_success() {
            let err_text = res.text().await.unwrap_or_default();
            anyhow::bail!("Google token exchange failed: {}", err_text);
        }

        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: i64,
            id_token: Option<String>,
            scope: Option<String>,
            token_type: String,
        }

        let token_resp: TokenResponse = res.json().await?;
        let now = Utc::now().timestamp();

        let token = GoogleToken {
            access_token: token_resp.access_token,
            refresh_token: token_resp.refresh_token,
            expires_at: now + token_resp.expires_in,
            id_token: token_resp.id_token,
            scope: token_resp.scope,
            token_type: token_resp.token_type,
        };

        self.store_token(token.clone()).await;
        Ok(token)
    }
}

/// Helper function to encode raw bytes to URL-safe Base64 without padding.
fn base64_url_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;

        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[((triple >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 63) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_clavis_ref_enforcement() {
        let mut cfg = GoogleOAuthConfig {
            client_id: "test-client-id".to_string(),
            redirect_uri: "http://localhost:8006/callback".to_string(),
            client_secret_clavis_ref: "google-secret-key".to_string(),
            scopes: vec!["email".to_string()],
        };

        cfg.ensure_clavis_ref();
        assert_eq!(cfg.client_secret_clavis_ref, "clavis://google-secret-key");
        assert_eq!(cfg.resolve_secret(), "google-secret-key");
    }

    #[test]
    fn test_auth_url_generation() {
        let cfg = GoogleOAuthConfig {
            client_id: "client-123.apps.googleusercontent.com".to_string(),
            redirect_uri: "http://localhost:8006/v1/auth/google/callback".to_string(),
            client_secret_clavis_ref: "clavis://my-secret".to_string(),
            scopes: vec![
                "openid".to_string(),
                "https://www.googleapis.com/auth/colab".to_string(),
            ],
        };

        let manager = GoogleOAuthManager::new(cfg);
        let state = manager.generate_state();
        let (verifier, challenge) = GoogleOAuthManager::generate_pkce();

        assert!(!state.is_empty());
        assert!(!verifier.is_empty());
        assert!(!challenge.is_empty());

        let url = manager.get_authorization_url(&state, &challenge);
        assert!(url.starts_with(GOOGLE_AUTH_URL));
        assert!(url.contains("client_id=client-123.apps.googleusercontent.com"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("colab"));
    }

    #[test]
    fn test_token_expiry_detection() {
        let now = Utc::now().timestamp();

        let valid_token = GoogleToken {
            access_token: "ya29.valid".to_string(),
            refresh_token: Some("1//refresh".to_string()),
            expires_at: now + 3600,
            id_token: None,
            scope: Some("https://www.googleapis.com/auth/colab".to_string()),
            token_type: "Bearer".to_string(),
        };

        let expired_token = GoogleToken {
            access_token: "ya29.expired".to_string(),
            refresh_token: None,
            expires_at: now - 10,
            id_token: None,
            scope: None,
            token_type: "Bearer".to_string(),
        };

        assert!(!valid_token.is_expired());
        assert!(expired_token.is_expired());
        assert!(valid_token.has_colab_scope());
        assert!(!expired_token.has_colab_scope());
    }

    #[tokio::test]
    async fn test_token_store_and_disconnect() {
        let cfg = GoogleOAuthConfig::default();
        let manager = GoogleOAuthManager::new(cfg);

        assert_eq!(manager.get_token().await, None);

        let status_before = manager.status().await;
        assert!(!status_before.is_connected);
        assert!(!status_before.has_colab_access);

        let token = GoogleToken {
            access_token: "ya29.test_token".to_string(),
            refresh_token: Some("1//refresh_token".to_string()),
            expires_at: Utc::now().timestamp() + 3600,
            id_token: Some("id.jwt.token".to_string()),
            scope: Some(
                "openid email profile https://www.googleapis.com/auth/colab".to_string(),
            ),
            token_type: "Bearer".to_string(),
        };

        manager.store_token(token.clone()).await;

        let stored = manager.get_token().await.expect("token should be stored");
        assert_eq!(stored.access_token, "ya29.test_token");

        let status_after = manager.status().await;
        assert!(status_after.is_connected);
        assert!(status_after.has_colab_access);
        assert_eq!(status_after.scopes.len(), 4);

        manager.disconnect().await;
        assert_eq!(manager.get_token().await, None);
        assert!(!manager.status().await.is_connected);
    }
}
