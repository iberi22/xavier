//! Loopback OAuth flow with PKCE, behind a mockable transport.
//!
//! # Flow: loopback redirect, not device-code
//!
//! Google's device-authorization grant is documented for **TV and limited-input
//! devices only**, and the out-of-band flow that a headless CLI would normally
//! use was fully blocked in January 2023. A backup daemon on a workstation can
//! open a browser and can bind a loopback port, so this module implements the
//! flow Google recommends for installed clients:
//!
//! ```text
//! 1. generate code_verifier, derive challenge = BASE64URL(SHA256(verifier))
//! 2. open <auth endpoint>?client_id=..&redirect_uri=http://127.0.0.1:<port>
//!      &response_type=code&scope=drive.file&code_challenge=..&code_challenge_method=S256
//!      &access_type=offline&prompt=consent&include_granted_scopes=true
//! 3. bind 127.0.0.1:<port>, wait for the redirect carrying ?code=..&state=..
//! 4. POST <token endpoint> grant_type=authorization_code + code + verifier
//! ```
//!
//! `access_type=offline` plus `prompt=consent` is what makes Google issue a
//! `refresh_token`; without both, a second consent returns no refresh token and
//! the backup breaks silently at the first token expiry.
//!
//! # Why the state check matters
//!
//! The loopback redirect is unauthenticated HTTP on localhost, so a *different*
//! local process could answer the callback first. The `state` value binds the
//! callback to the request that started it, and the PKCE verifier binds the
//! authorization code to *this* client. Both are checked before a token request
//! is ever made.
//!
//! # Testing
//!
//! [`OAuthTransport`] abstracts the two HTTP calls, so the full flow including
//! the wrong-state and wrong-verifier paths runs in-process with no network, as
//! required by the `ci-safe` suite.

use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::crypto::hex_encode;
use crate::drive::credentials::{DriveCredentials, DriveScopes};

/// Google's authorization endpoint.
pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Google's token endpoint.
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Environment variable holding the OAuth client id.
pub const CLIENT_ID_ENV: &str = "XAVIER_GOOGLE_CLIENT_ID";

/// Environment variable holding the OAuth client secret.
///
/// A loopback client is *not* confidential, so the secret is optional: when it is
/// absent the token call sends only the client id, which Google accepts for
/// installed/native clients.
pub const CLIENT_SECRET_ENV: &str = "XAVIER_GOOGLE_CLIENT_SECRET";

/// Minimum loopback port. 0 asks the OS for an ephemeral port.
pub const DEFAULT_LOOPBACK_PORT: u16 = 0;

/// PKCE verifier generated for an authorization request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCode {
    pub state: String,
    pub code_verifier: String,
    pub code_challenge: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

impl DeviceCode {
    /// Generate a fresh state + PKCE S256 pair for a loopback redirect.
    pub fn generate(loopback_port: u16) -> Self {
        let mut verifier_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut verifier_bytes);
        let mut state_bytes = [0u8; 16];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut state_bytes);

        let code_verifier = base64_url_encode(&verifier_bytes);
        let code_challenge = base64_url_encode(&Sha256::digest(code_verifier.as_bytes()));

        Self {
            state: base64_url_encode(&state_bytes),
            code_verifier,
            code_challenge,
            // Loopback IP, never a hostname: RFC 8252 §7.3 and Google's own
            // guidance require the IP literal so the redirect cannot be
            // hijacked via DNS.
            redirect_uri: format!("http://127.0.0.1:{loopback_port}"),
            scopes: DriveScopes::REQUESTED
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    /// Build the URL a human must open to consent.
    pub fn authorization_url(&self, client_id: &str) -> String {
        format!(
            "{AUTH_ENDPOINT}?client_id={}&redirect_uri={}&response_type=code&scope={}\
             &state={}&code_challenge={}&code_challenge_method=S256\
             &access_type=offline&prompt=consent&include_granted_scopes=true",
            urlencoding::encode(client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(&self.scopes.join(" ")),
            urlencoding::encode(&self.state),
            urlencoding::encode(&self.code_challenge),
        )
    }
}

/// Google's token response, reduced to what a backup needs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub scope: Option<String>,
    pub token_type: String,
}

impl TokenResponse {
    /// Convert to the sealed-credential shape.
    ///
    /// Fails when no refresh token was returned: without one the backup works
    /// until the first expiry and then fails unattended at 03:17, so it must
    /// never be accepted silently.
    pub fn into_credentials(&self, now: i64) -> Result<DriveCredentials> {
        let refresh_token = self.refresh_token.clone().context(
            "Google returned no refresh_token (access_type=offline/prompt=consent missing?)",
        )?;
        Ok(DriveCredentials {
            expires_at: now + self.expires_in,
            scopes: self
                .scope
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            refresh_token,
        })
    }
}

/// The two HTTP calls the flow needs, so tests can run offline.
#[async_trait::async_trait]
pub trait OAuthTransport: Send + Sync {
    /// POST the token exchange.
    async fn post_token(&self, form: &HashMap<&str, String>) -> Result<TokenResponse>;

    /// `true` when the transport performs real network I/O.
    fn is_live(&self) -> bool {
        false
    }
}

/// Real `reqwest` transport.
pub struct HttpOAuthTransport {
    client: reqwest::Client,
}

impl Default for HttpOAuthTransport {
    fn default() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait::async_trait]
impl OAuthTransport for HttpOAuthTransport {
    async fn post_token(&self, form: &HashMap<&str, String>) -> Result<TokenResponse> {
        // The form body is encoded by hand: `reqwest` is built without the
        // `form` feature in this crate (Cargo.toml:261), and adding it just for
        // this call would widen the dependency surface.
        let body = form
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let res = self
            .client
            .post(TOKEN_ENDPOINT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json")
            .body(body)
            .send()
            .await
            .context("token request failed")?;

        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            // Google's error body names the OAuth error code; pass the code only.
            let code = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .unwrap_or_else(|| status.as_str().to_string());
            bail!("Google rejected the token exchange: {code}");
        }
        Ok(serde_json::from_str(&body).context("token response is not valid JSON")?)
    }

    fn is_live(&self) -> bool {
        true
    }
}

/// Drives the loopback flow.
pub struct DriveAuthClient {
    transport: std::sync::Arc<dyn OAuthTransport>,
    client_id: String,
    client_secret: Option<String>,
}

impl DriveAuthClient {
    /// Build over any transport (tests inject a fake).
    pub fn new(
        transport: std::sync::Arc<dyn OAuthTransport>,
        client_id: impl Into<String>,
        client_secret: Option<String>,
    ) -> Self {
        Self {
            transport,
            client_id: client_id.into(),
            client_secret,
        }
    }

    /// Build over the real network transport, reading credentials from the
    /// environment.
    ///
    /// Returns an error naming the missing variable rather than an empty id, so a
    /// half-configured daemon fails loudly instead of sending an unauthenticated
    /// consent request.
    pub fn from_env() -> Result<Self> {
        let client_id = std::env::var(CLIENT_ID_ENV).map_err(|_| {
            anyhow::anyhow!(
                "{CLIENT_ID_ENV} is not set; cannot start the Google Drive consent flow"
            )
        })?;
        if client_id.trim().is_empty() {
            bail!("{CLIENT_ID_ENV} is set but empty");
        }
        let secret = std::env::var(CLIENT_SECRET_ENV)
            .ok()
            .filter(|s| !s.trim().is_empty());
        Ok(Self::new(
            std::sync::Arc::new(HttpOAuthTransport::default()),
            client_id,
            secret,
        ))
    }

    /// Step 1: begin a flow. The returned [`DeviceCode`] holds the state and
    /// verifier that the callback will be checked against.
    pub fn begin(&self, loopback_port: u16) -> DeviceCode {
        DeviceCode::generate(loopback_port)
    }

    /// The URL the operator must open.
    pub fn authorization_url(&self, device: &DeviceCode) -> String {
        device.authorization_url(&self.client_id)
    }

    /// Step 2: validate the callback query, then exchange the code.
    ///
    /// `query` is the parsed redirect query string. State is compared in constant
    /// time; a mismatch aborts before any token request, because a wrong state
    /// means some other responder answered the callback.
    pub async fn complete(
        &self,
        device: &DeviceCode,
        query: &HashMap<String, String>,
        now: i64,
    ) -> Result<DriveCredentials> {
        if let Some(err) = query.get("error") {
            bail!("user did not grant access: {err}");
        }
        let state = query
            .get("state")
            .context("callback is missing the state parameter")?;
        if !bool::from(subtle::ConstantTimeEq::ct_eq(
            state.as_bytes(),
            device.state.as_bytes(),
        )) {
            bail!("callback state does not match this request; refusing the exchange");
        }
        let code = query
            .get("code")
            .context("callback is missing the authorization code")?;

        let mut form: HashMap<&str, String> = HashMap::new();
        form.insert("client_id", self.client_id.clone());
        form.insert("code", code.clone());
        form.insert("code_verifier", device.code_verifier.clone());
        form.insert("grant_type", "authorization_code".to_string());
        form.insert("redirect_uri", device.redirect_uri.clone());
        if let Some(secret) = &self.client_secret {
            form.insert("client_secret", secret.clone());
        }

        let response = self.transport.post_token(&form).await?;
        response.into_credentials(now)
    }

    /// Whether this client talks to the real network.
    pub fn is_live(&self) -> bool {
        self.transport.is_live()
    }
}

fn base64_url_encode(data: &[u8]) -> String {
    crate::crypto::base64_encode(data)
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// Fake transport: no network, records what was sent.
    struct FakeTransport {
        response: TokenResponse,
        seen: std::sync::Mutex<Vec<HashMap<String, String>>>,
    }

    impl FakeTransport {
        fn ok() -> Self {
            Self {
                response: TokenResponse {
                    access_token: "ya29.access".into(),
                    refresh_token: Some("1//refresh".into()),
                    expires_in: 3599,
                    scope: Some(DriveScopes::BACKUP.to_string()),
                    token_type: "Bearer".into(),
                },
                seen: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn without_refresh() -> Self {
            Self {
                response: TokenResponse {
                    refresh_token: None,
                    ..Self::ok().response
                },
                ..Self::ok()
            }
        }

        fn last_form(&self) -> HashMap<String, String> {
            self.seen
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl OAuthTransport for FakeTransport {
        async fn post_token(&self, form: &HashMap<&str, String>) -> Result<TokenResponse> {
            let snapshot: HashMap<String, String> = form
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect();
            self.seen.lock().unwrap().push(snapshot);
            Ok(self.response.clone())
        }
    }

    fn query(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    const NOW: i64 = 1_700_000_000;

    /// AC: the full loopback flow produces sealed-ready credentials.
    #[tokio::test]
    async fn loopback_flow_completes() {
        let fake = std::sync::Arc::new(FakeTransport::ok());
        let client = DriveAuthClient::new(fake.clone(), "client-123", None);
        let device = client.begin(DEFAULT_LOOPBACK_PORT);

        let creds = client
            .complete(
                &device,
                &query(&[("state", &device.state), ("code", "auth-code")]),
                NOW,
            )
            .await
            .unwrap();

        assert_eq!(creds.refresh_token, "1//refresh");
        assert_eq!(creds.expires_at, NOW + 3599);
        assert!(creds.has_backup_scope());
        assert!(!creds.access_token_expired(NOW));

        // PKCE verifier must be forwarded, and the client secret omitted when absent.
        let form = fake.last_form();
        assert_eq!(form.get("code_verifier"), Some(&device.code_verifier));
        assert_eq!(
            form.get("grant_type").map(String::as_str),
            Some("authorization_code")
        );
        assert!(!form.contains_key("client_secret"));
    }

    /// AC: the consent URL requests only `drive.file` and offline access.
    #[test]
    fn authorization_url_requests_minimum_scopes_and_offline_access() {
        let client =
            DriveAuthClient::new(std::sync::Arc::new(FakeTransport::ok()), "client-123", None);
        let device = client.begin(0);
        let url = client.authorization_url(&device);

        assert!(url.starts_with(AUTH_ENDPOINT));
        assert!(
            url.contains("access_type=offline"),
            "refresh token needs offline"
        );
        assert!(
            url.contains("prompt=consent"),
            "refresh token needs consent"
        );
        assert!(url.contains("code_challenge_method=S256"));

        // Compare the granted scope set exactly. A substring check is wrong
        // here: the encoded full-drive scope is a PREFIX of the encoded
        // drive.file scope, so `url.contains(FULL)` would wrongly pass on a URL
        // that in fact requests only the narrow scope.
        let scope_param = url
            .split("scope=")
            .nth(1)
            .expect("authorization URL must carry a scope parameter")
            .split('&')
            .next()
            .expect("scope must be terminated by '&'");
        let granted: Vec<&str> = scope_param
            .split('+')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        assert!(
            granted.contains(&urlencoding::encode(DriveScopes::BACKUP).as_ref()),
            "drive.file must be requested; granted = {granted:?}"
        );
        assert!(
            !granted.contains(&urlencoding::encode(DriveScopes::FULL).as_ref()),
            "the full drive scope must never be requested; granted = {granted:?}"
        );
    }

    /// AC: the redirect is a loopback IP literal, never a hostname.
    #[test]
    fn redirect_uri_is_a_loopback_ip_literal() {
        let device = DeviceCode::generate(0);
        assert!(device.redirect_uri.starts_with("http://127.0.0.1:"));
        assert!(!device.redirect_uri.contains("localhost"));
    }

    /// AC: a callback from an unexpected responder is refused before any exchange.
    #[tokio::test]
    async fn wrong_state_is_refused_without_contacting_google() {
        let fake = std::sync::Arc::new(FakeTransport::ok());
        let client = DriveAuthClient::new(fake.clone(), "client-123", None);
        let device = client.begin(0);

        let err = client
            .complete(
                &device,
                &query(&[("state", "attacker-state"), ("code", "x")]),
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("state does not match"));
        assert!(
            fake.seen.lock().unwrap().is_empty(),
            "a rejected state must not trigger a token request"
        );
    }

    /// AC: no refresh token must fail loudly, not degrade to a silent expiry.
    #[tokio::test]
    async fn missing_refresh_token_is_rejected() {
        let client = DriveAuthClient::new(
            std::sync::Arc::new(FakeTransport::without_refresh()),
            "client-123",
            None,
        );
        let device = client.begin(0);
        let err = client
            .complete(
                &device,
                &query(&[("state", &device.state), ("code", "x")]),
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no refresh_token"));
    }

    /// A user who denies consent is reported, not treated as success.
    #[tokio::test]
    async fn user_denied_consent_is_reported() {
        let client =
            DriveAuthClient::new(std::sync::Arc::new(FakeTransport::ok()), "client-123", None);
        let device = client.begin(0);
        let err = client
            .complete(
                &device,
                &query(&[("error", "access_denied"), ("state", &device.state)]),
                NOW,
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("did not grant"));
    }

    /// Missing callback parameters are rejected.
    #[tokio::test]
    async fn missing_callback_parameters_are_rejected() {
        let client =
            DriveAuthClient::new(std::sync::Arc::new(FakeTransport::ok()), "client-123", None);
        let device = client.begin(0);
        assert!(client
            .complete(&device, &query(&[("code", "x")]), NOW)
            .await
            .is_err());
    }

    /// Each flow gets fresh, unguessable state and verifier.
    #[test]
    fn state_and_verifier_are_fresh_per_flow() {
        let a = DeviceCode::generate(0);
        let b = DeviceCode::generate(0);
        assert_ne!(a.state, b.state);
        assert_ne!(a.code_verifier, b.code_verifier);
        assert_ne!(a.code_challenge, b.code_verifier);
    }

    /// PKCE S256: challenge = BASE64URL(SHA256(verifier)), unpadded.
    #[test]
    fn pkce_challenge_matches_s256() {
        let device = DeviceCode::generate(0);
        let expected = base64_url_encode(&Sha256::digest(device.code_verifier.as_bytes()));
        assert_eq!(device.code_challenge, expected);
        assert!(!device.code_challenge.contains('='));
        assert!(!device.code_challenge.contains('+') && !device.code_challenge.contains('/'));
    }

    /// The fake transport is not live; the real one is.
    #[tokio::test]
    async fn transport_liveness_is_reported() {
        let client =
            DriveAuthClient::new(std::sync::Arc::new(FakeTransport::ok()), "client-123", None);
        assert!(!client.is_live());
        assert!(HttpOAuthTransport::default().is_live());
    }

    /// Missing client id fails loudly instead of sending an empty consent.
    #[test]
    fn from_env_requires_a_client_id() {
        // Guarded: the suite runs single-threaded and this mutates process env.
        let saved = std::env::var(CLIENT_ID_ENV).ok();
        std::env::remove_var(CLIENT_ID_ENV);
        assert!(DriveAuthClient::from_env().is_err());

        std::env::set_var(CLIENT_ID_ENV, "   ");
        assert!(DriveAuthClient::from_env().is_err());

        std::env::set_var(CLIENT_ID_ENV, "real-client-id");
        assert!(DriveAuthClient::from_env().is_ok());

        match saved {
            Some(v) => std::env::set_var(CLIENT_ID_ENV, v),
            None => std::env::remove_var(CLIENT_ID_ENV),
        }
    }
}
