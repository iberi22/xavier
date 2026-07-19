//! Authentication Module for Xavier
//! JWT-based authentication, RBAC, and TOTP support

use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use qrcode::render::unicode;
use qrcode::QrCode;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fmt;
use totp_rs::{Algorithm as TotpAlgorithm, Secret, TOTP};

use crate::security::auth_store::AuthStore;

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
    pub fn new(issuer: &str) -> Self {
        Self {
            issuer: issuer.to_string(),
        }
    }

    pub fn generate_secret(&self) -> String {
        Secret::generate_secret().to_encoded().to_string()
    }

    pub fn get_qr_code(&self, account_name: &str, secret_base32: &str) -> Result<String> {
        let totp = TOTP::new(
            TotpAlgorithm::SHA1,
            6,
            1,
            30,
            Secret::Encoded(secret_base32.to_string())
                .to_bytes()
                .map_err(|_| anyhow!("invalid secret"))?,
            Some(self.issuer.clone()),
            account_name.to_string(),
        )
        .map_err(|e| anyhow!("TOTP init failed: {}", e))?;

        let code = totp.get_url();
        let qr = QrCode::new(code.as_bytes())?;
        Ok(qr.render::<unicode::Dense1x2>().build())
    }

    pub fn verify_code(&self, secret_base32: &str, code: &str) -> bool {
        let secret = match Secret::Encoded(secret_base32.to_string()).to_raw() {
            Ok(s) => s,
            Err(_) => return false,
        };

        let totp = match TOTP::new(
            TotpAlgorithm::SHA1,
            6,
            1,
            30,
            secret.to_bytes().unwrap_or_default(),
            Some(self.issuer.clone()),
            "user".to_string(),
        ) {
            Ok(t) => t,
            Err(_) => return false,
        };

        totp.check_current(code).unwrap_or(false)
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
}

/// Resolves the Xavier token from environment variable
pub fn resolve_xavier_token() -> String {
    std::env::var("XAVIER_TOKEN").unwrap_or_default()
}
