//! Authentication Module for Xavier
//! JWT-based authentication, RBAC, and TOTP support

use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use qrcode::render::unicode;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::fmt;
use totp_rs::{Algorithm as TotpAlgorithm, Builder, Secret, Totp};

/// JWT Claims for authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,    // User ID
    pub email: String,  // User email
    pub role: UserRole, // User role
    pub exp: i64,       // Expiration timestamp
    pub iat: i64,       // Issued at
}

impl Claims {
    /// New.
    pub fn new(user_id: String, email: String, role: UserRole, expires_in: Duration) -> Self {
        let now = Utc::now();
        Self {
            sub: user_id,
            email,
            role,
            exp: (now + expires_in).timestamp(),
            iat: now.timestamp(),
        }
    }
}

/// User roles for RBAC
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    #[default]
    User,
    Readonly,
}

/// User representation
#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub role: UserRole,
    pub api_key: String, // Deprecated but kept for compatibility
    pub created_at: i64,
    pub updated_at: i64,
}

impl User {
    /// New.
    pub fn new(email: String, name: String, role: UserRole) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id: ulid::Ulid::new().to_string(),
            email,
            name,
            role,
            api_key: format!("sk-{}", ulid::Ulid::new().to_string().to_lowercase()),
            created_at: now,
            updated_at: now,
        }
    }
}

impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("name", &self.name)
            .field("role", &self.role)
            .field("api_key", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// Token Generation
pub fn generate_jwt(user: &User, secret: &[u8]) -> Result<String> {
    let claims = Claims::new(
        user.id.clone(),
        user.email.clone(),
        user.role,
        Duration::hours(1),
    );

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
    .map_err(|e| anyhow!("JWT encoding failed: {}", e))
}

/// Validate jwt.
pub fn validate_jwt(token: &str, secret: &[u8]) -> Result<Claims> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret),
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|e| anyhow!("JWT validation failed: {}", e))?;

    Ok(token_data.claims)
}

/// TOTP Support
pub struct TotpProvider {
    issuer: String,
}

impl TotpProvider {
    /// New.
    pub fn new(issuer: &str) -> Self {
        Self {
            issuer: issuer.to_string(),
        }
    }

    /// Generate secret.
    pub fn generate_secret(&self) -> String {
        Secret::generate().to_base32()
    }

    /// Get qr code.
    pub fn get_qr_code(&self, account_name: &str, secret_base32: &str) -> Result<String> {
        let secret =
            Secret::try_from_base32(secret_base32).map_err(|_| anyhow!("invalid secret"))?;
        let totp = Builder::new()
            .with_algorithm(TotpAlgorithm::SHA1)
            .with_digits(6)
            .with_skew(1)
            .with_step_duration(30)
            .with_secret(secret)
            .with_account_name(account_name.to_string())
            .with_issuer(Some(self.issuer.clone()))
            .build()
            .map_err(|e| anyhow!("TOTP init failed: {e}"))?;

        let code = totp.to_url().map_err(|e| anyhow!("TOTP url failed: {e}"))?;
        let qr = QrCode::new(code.as_bytes())?;
        Ok(qr.render::<unicode::Dense1x2>().build())
    }

    /// Verify code.
    pub fn verify_code(&self, secret_base32: &str, code: &str) -> bool {
        let secret = match Secret::try_from_base32(secret_base32) {
            Ok(s) => s,
            Err(_) => return false,
        };

        let totp = match Builder::new()
            .with_algorithm(TotpAlgorithm::SHA1)
            .with_digits(6)
            .with_skew(1)
            .with_step_duration(30)
            .with_secret(secret)
            .with_account_name("user".to_string())
            .with_issuer(Some(self.issuer.clone()))
            .build()
        {
            Ok(t) => t,
            Err(_) => return false,
        };

        totp.check_current(code).is_some()
    }
}

/// Permission check
pub trait Permission {
    fn can_view_dashboard(&self) -> bool;
    fn can_search_memory(&self) -> bool;
    fn can_add_memory(&self) -> bool;
    fn can_delete_memory(&self) -> bool;
    fn can_manage_beliefs(&self) -> bool;
    fn can_run_agents(&self) -> bool;
    fn can_view_config(&self) -> bool;
    fn can_edit_config(&self) -> bool;
    fn can_manage_users(&self) -> bool;
    /// Lend, execute with, or inspect node secrets.
    ///
    /// Admin only, and deliberately not reachable by any scope: handing a
    /// secret to a process is an owner decision, not something a token
    /// scope can buy.
    fn can_manage_secrets(&self) -> bool;
}

impl Permission for UserRole {
    fn can_view_dashboard(&self) -> bool {
        true
    }
    fn can_search_memory(&self) -> bool {
        true
    }
    fn can_add_memory(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::User)
    }
    fn can_delete_memory(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::User)
    }
    fn can_manage_beliefs(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::User)
    }
    fn can_run_agents(&self) -> bool {
        matches!(self, UserRole::Admin | UserRole::User)
    }
    fn can_view_config(&self) -> bool {
        true
    }
    fn can_edit_config(&self) -> bool {
        matches!(self, UserRole::Admin)
    }
    fn can_manage_users(&self) -> bool {
        matches!(self, UserRole::Admin)
    }
    fn can_manage_secrets(&self) -> bool {
        matches!(self, UserRole::Admin)
    }
}

/// Token configuration health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenConfigStatus {
    Valid,
    Unset,
    SuspectComment,
    #[serde(alias = "vault")]
    VaultBacked,
}

impl TokenConfigStatus {
    /// Canonical name of the source providing the token configuration.
    pub fn source(&self) -> &'static str {
        match self {
            Self::Valid => "environment",
            Self::Unset => "none",
            Self::SuspectComment => "environment_comment",
            Self::VaultBacked => "vault",
        }
    }
}

impl std::fmt::Display for TokenConfigStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid => write!(f, "valid"),
            Self::Unset => write!(f, "unset"),
            Self::SuspectComment => write!(f, "suspect_comment"),
            Self::VaultBacked => write!(f, "vault_backed"),
        }
    }
}

/// Dedicated vault entry name for the Xavier token.
pub const XAVIER_TOKEN_VAULT_ENTRY: &str = "XAVIER_TOKEN";

/// Service name of the production secrets vault.
const PRODUCTION_VAULT_SERVICE: &str = "xavier";

#[cfg(test)]
static TEST_VAULT: std::sync::RwLock<Option<std::sync::Arc<crate::secrets::vault::HardwareVault>>> =
    std::sync::RwLock::new(None);

#[cfg(test)]
pub(crate) struct TestVaultGuard;

#[cfg(test)]
impl Drop for TestVaultGuard {
    fn drop(&mut self) {
        if let Ok(mut guard) = TEST_VAULT.write() {
            *guard = None;
        }
    }
}

#[cfg(test)]
pub(crate) fn with_test_vault(vault: crate::secrets::vault::HardwareVault) -> TestVaultGuard {
    if let Ok(mut guard) = TEST_VAULT.write() {
        *guard = Some(std::sync::Arc::new(vault));
    }
    TestVaultGuard
}

/// Resolves the token from the hardware vault if present and non-empty.
///
/// Test builds never fall back to the production vault: without an active
/// `with_test_vault` guard this returns `None` instead of touching the real
/// OS keyring or `~/.xavier/secrets`.
#[cfg(test)]
fn get_vault_token() -> Option<String> {
    let guard = TEST_VAULT.read().ok()?;
    let vault = guard.as_ref()?;
    vault
        .get_secret(XAVIER_TOKEN_VAULT_ENTRY)
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// Resolves the token from the hardware vault if present and non-empty.
#[cfg(not(test))]
fn get_vault_token() -> Option<String> {
    crate::secrets::vault::HardwareVault::new(PRODUCTION_VAULT_SERVICE)
        .get_secret(XAVIER_TOKEN_VAULT_ENTRY)
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// Inspects current XAVIER_TOKEN configuration
pub fn inspect_xavier_token() -> (TokenConfigStatus, Option<&'static str>) {
    match std::env::var("XAVIER_TOKEN") {
        Ok(t) if t.contains('#') => (
            TokenConfigStatus::SuspectComment,
            Some("XAVIER_TOKEN contains a '#' character. Check for unquoted inline comments in your .env file."),
        ),
        Ok(t) if !t.trim().is_empty() => (TokenConfigStatus::Valid, None),
        Ok(_) => {
            if get_vault_token().is_some() {
                (TokenConfigStatus::VaultBacked, None)
            } else {
                (
                    TokenConfigStatus::Unset,
                    Some("XAVIER_TOKEN is empty. Set a non-empty token in your environment or .env file."),
                )
            }
        }
        Err(_) => {
            if get_vault_token().is_some() {
                (TokenConfigStatus::VaultBacked, None)
            } else {
                (
                    TokenConfigStatus::Unset,
                    Some("XAVIER_TOKEN is unset in the environment. Configure XAVIER_TOKEN in .env or systemd EnvironmentFile."),
                )
            }
        }
    }
}

/// Resolves the Xavier token from environment variable or hardware vault
pub fn resolve_xavier_token() -> String {
    let (status, warning) = inspect_xavier_token();
    if let Some(msg) = warning {
        tracing::warn!(token_status = ?status, "{msg}");
    }
    match std::env::var("XAVIER_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => get_vault_token().unwrap_or_default(),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthHealth {
    pub token_config: TokenConfigStatus,
}

impl Default for AuthHealth {
    fn default() -> Self {
        let (status, _) = inspect_xavier_token();
        Self {
            token_config: status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspect_xavier_token_branches() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = crate::secrets::vault::HardwareVault::new("xavier")
            .isolated(temp_dir.path().join("secrets"), [42u8; 32]);
        let _vault_guard = with_test_vault(vault);

        // Valid token
        std::env::set_var("XAVIER_TOKEN", "valid-secret-token");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::Valid);
        assert!(warn.is_none());

        // Empty token
        std::env::set_var("XAVIER_TOKEN", "   ");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::Unset);
        assert!(warn.is_some());

        // Suspect comment token (# in token)
        std::env::set_var("XAVIER_TOKEN", "secret-token # inline comment");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::SuspectComment);
        assert!(warn.is_some());

        // Unset
        std::env::remove_var("XAVIER_TOKEN");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::Unset);
        assert!(warn.is_some());

        // Restore
        std::env::set_var("XAVIER_TOKEN", "test-token");
    }

    #[test]
    fn test_resolve_xavier_token_prefers_environment() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        std::env::set_var("XAVIER_TOKEN", "env-token-priority");

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = crate::secrets::vault::HardwareVault::new("xavier")
            .isolated(temp_dir.path().join("secrets"), [42u8; 32]);
        vault
            .store_secret(XAVIER_TOKEN_VAULT_ENTRY, "vault-token-secondary")
            .expect("store secret");
        let _vault_guard = with_test_vault(vault);

        assert_eq!(resolve_xavier_token(), "env-token-priority");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::Valid);
        assert!(warn.is_none());
    }

    #[test]
    fn test_resolve_xavier_token_falls_back_to_vault() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        std::env::remove_var("XAVIER_TOKEN");

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = crate::secrets::vault::HardwareVault::new("xavier")
            .isolated(temp_dir.path().join("secrets"), [42u8; 32]);
        vault
            .store_secret(XAVIER_TOKEN_VAULT_ENTRY, "vault-token-fallback")
            .expect("store secret");
        let _vault_guard = with_test_vault(vault);

        assert_eq!(resolve_xavier_token(), "vault-token-fallback");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::VaultBacked);
        assert!(warn.is_none());

        // Also test when environment variable is present but empty / blank
        std::env::set_var("XAVIER_TOKEN", "   ");
        assert_eq!(resolve_xavier_token(), "vault-token-fallback");
        let (status_blank, warn_blank) = inspect_xavier_token();
        assert_eq!(status_blank, TokenConfigStatus::VaultBacked);
        assert!(warn_blank.is_none());
    }

    #[test]
    fn test_resolve_xavier_token_returns_empty_when_absent() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        std::env::remove_var("XAVIER_TOKEN");

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = crate::secrets::vault::HardwareVault::new("xavier")
            .isolated(temp_dir.path().join("secrets"), [42u8; 32]);
        let _vault_guard = with_test_vault(vault);

        assert_eq!(resolve_xavier_token(), "");
        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::Unset);
        assert!(warn.is_some());
    }

    #[test]
    fn test_inspect_xavier_token_reports_vault_source() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        std::env::remove_var("XAVIER_TOKEN");

        let temp_dir = tempfile::tempdir().expect("tempdir");
        let vault = crate::secrets::vault::HardwareVault::new("xavier")
            .isolated(temp_dir.path().join("secrets"), [42u8; 32]);
        let synthetic_token = "synthetic-canary-secret";
        vault
            .store_secret(XAVIER_TOKEN_VAULT_ENTRY, synthetic_token)
            .expect("store secret");
        let _vault_guard = with_test_vault(vault);

        let (status, warn) = inspect_xavier_token();
        assert_eq!(status, TokenConfigStatus::VaultBacked);
        assert!(warn.is_none());
        assert_eq!(status.source(), "vault");

        let debug_repr = format!("{status:?}");
        assert!(
            debug_repr.to_lowercase().contains("vault"),
            "status must name the vault source"
        );
        assert!(
            !debug_repr.contains(synthetic_token),
            "status must never contain the token"
        );

        let display_repr = format!("{status}");
        assert!(
            display_repr.contains("vault"),
            "display status must name the vault source"
        );
        assert!(
            !display_repr.contains(synthetic_token),
            "display status must never contain the token"
        );

        let serialized = serde_json::to_string(&status).expect("serialize status");
        assert!(
            serialized.contains("vault"),
            "serialized status must name the vault source"
        );
        assert!(
            !serialized.contains(synthetic_token),
            "serialized status must never contain the token"
        );
    }

    #[test]
    fn test_totp_generate_verify_roundtrip() {
        let provider = TotpProvider::new("XavierTest");
        let secret_b32 = provider.generate_secret();
        assert!(!secret_b32.is_empty());

        // Mint a current code with an equivalent Totp instance
        let secret = Secret::try_from_base32(&secret_b32).expect("valid base32");
        let totp: Totp = Builder::new()
            .with_algorithm(TotpAlgorithm::SHA1)
            .with_digits(6)
            .with_skew(1)
            .with_step_duration(30)
            .with_secret(secret)
            .with_account_name("user".to_string())
            .with_issuer(Some("XavierTest".to_string()))
            .build()
            .expect("build TOTP");
        let code = totp.generate_current().to_string();

        assert!(provider.verify_code(&secret_b32, &code));
        assert!(!provider.verify_code(&secret_b32, "000000"));
        assert!(!provider.verify_code("!!!not-base32!!!", &code));
    }
}
