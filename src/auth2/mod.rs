//! Authentication and authorization module.
//!
//! Provides JWT-based authentication, password hashing/verification,
//! refresh token rotation, and RBAC middleware for securing API endpoints.
//! Includes database-backed session and user stores.

pub mod db;
pub mod jwt;
pub mod middleware;
pub mod password;
pub mod refresh;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use qrcode::render::unicode; // usar unicode QR para evitar dependencia render svg
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use totp_rs::{Algorithm as TOTPAlgorithm, Secret, TOTP};
// use crate::cli::server::CliState;
use crate::auth2::db::{AuditLog, AuthDb, User};
use crate::auth2::jwt::JwtManager;
use crate::auth2::password::{hash_password, verify_password};
use crate::auth2::refresh::RefreshTokenManager;
use anyhow::Result;

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub totp_code: Option<String>,
}

#[derive(Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct TwoFactorSetupRequest {} // Empty body, user_id from JWT claims

#[derive(Serialize)]
pub struct TwoFactorSetupResponse {
    pub qr_code: String,
    pub secret: String,
    pub backup_codes: Vec<String>,
}

#[derive(Deserialize)]
pub struct TwoFactorVerifyRequest {
    pub code: String,
}

#[derive(Deserialize)]
pub struct RecoveryRequest {
    pub email: String,
    pub seed_phrase: String,
    pub new_password: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub name: String,
    pub role: String,
    pub totp_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            role: user.role,
            totp_enabled: user.totp_enabled,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct RegisterResponse {
    pub user: UserResponse,
    pub seed_phrase: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserResponse,
    pub requires_2fa: bool,
}

pub trait HasAuthDb {
    fn auth_db(&self) -> Option<std::sync::Arc<parking_lot::Mutex<crate::auth2::db::AuthDb>>>;
}

pub fn auth_routes<S>(base_path: &str) -> Router<S>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    use crate::auth2::middleware::auth_middleware;
    use axum::middleware::from_fn;
    use tower::ServiceBuilder;

    // Protected routes (require JWT via middleware)
    let protected = Router::new()
        .route("/2fa/setup", post(setup_2fa_handler::<S>))
        .route("/2fa/verify", post(verify_2fa_handler::<S>))
        .route("/status", get(status_handler));
    let protected = protected.layer(ServiceBuilder::new().layer(from_fn(auth_middleware)));

    // Public + protected merged
    Router::new()
        .route("/register", post(register_handler::<S>))
        .route("/login", post(login_handler::<S>))
        .route("/refresh", post(refresh_handler::<S>))
        .route("/logout", post(logout_handler::<S>))
        .route("/check-users", get(check_users_handler::<S>))
        .route("/recovery", post(recovery_handler::<S>))
        .merge(protected)
        .layer(axum::Extension(std::sync::Arc::new(base_path.to_string())))
}

async fn register_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    Json(payload): Json<RegisterRequest>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();

    // Check if user exists
    if auth_db
        .get_user_by_email(&payload.email)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .is_some()
    {
        return Err(StatusCode::CONFLICT);
    }

    let password_hash =
        hash_password(&payload.password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Generate seed phrase for recovery
    let mnemonic = bip39::Mnemonic::generate_in(bip39::Language::Spanish, 24)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let seed_phrase_str = mnemonic.to_string();

    // Hash recovery seed phrase
    let seed_hash = {
        let mut hasher = Sha256::new();
        hasher.update(seed_phrase_str.as_bytes());
        crate::crypto::hex_encode(hasher.finalize())
    };

    let user = User {
        id: ulid::Ulid::new().to_string(),
        email: payload.email,
        password_hash,
        name: payload.name,
        role: "user".to_string(),
        totp_secret: None,
        totp_enabled: false,
        recovery_seed_hash: Some(seed_hash),
        backup_codes: None,
        created_at: now,
        updated_at: now,
    };

    auth_db
        .create_user(&user)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    auth_db
        .log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id.clone()),
            action: "register".to_string(),
            ip_address: None,
            details: None,
            created_at: now,
        })
        .ok();

    Ok(Json(RegisterResponse {
        seed_phrase: seed_phrase_str,
        user: UserResponse::from(user),
    }))
}

async fn login_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();

    let user = auth_db
        .get_user_by_email(&payload.email)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !verify_password(&payload.password, &user.password_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // TOTP check if enabled
    let requires_2fa = user.totp_enabled;
    if user.totp_enabled {
        let code = payload.totp_code.ok_or(StatusCode::UNAUTHORIZED)?;
        if let Some(ref secret) = user.totp_secret {
            let secret_bytes = Secret::Encoded(secret.clone())
                .to_bytes()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let totp = TOTP {
                algorithm: TOTPAlgorithm::SHA1,
                digits: 6,
                skew: 1,
                step: 30,
                secret: secret_bytes,
                issuer: Some("Xavier".to_string()),
                account_name: user.email.clone(),
            };
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / 30;
            if !totp.check(&code, time) {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
    }

    let jwt_manager = JwtManager::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let access_token = jwt_manager
        .create_token(&user.id, &user.email, &user.role)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let refresh_manager = RefreshTokenManager::new(&auth_db);
    let refresh_token = refresh_manager
        .generate_token(&user.id, None)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    auth_db
        .log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id.clone()),
            action: "login".to_string(),
            ip_address: None,
            details: None,
            created_at: now,
        })
        .ok();

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        user: UserResponse::from(user),
        requires_2fa,
    }))
}

async fn refresh_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    Json(payload): Json<RefreshRequest>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();
    let refresh_manager = RefreshTokenManager::new(&auth_db);

    let (new_refresh_token, user_id) = refresh_manager
        .rotate_token(&payload.refresh_token, None)
        .map_err(|e| {
        if e.to_string().contains("Potential theft detected") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        }
    })?;

    let user = auth_db
        .get_user_by_id(&user_id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_manager = JwtManager::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let access_token = jwt_manager
        .create_token(&user.id, &user.email, &user.role)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token: new_refresh_token,
    }))
}

async fn logout_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    Json(payload): Json<RefreshRequest>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();

    let hash = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(payload.refresh_token.as_bytes());
        crate::crypto::hex_encode(hasher.finalize())
    };

    if let Some(token) = auth_db
        .get_refresh_token_by_hash(&hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        auth_db
            .revoke_refresh_token(&token.id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        auth_db
            .log_audit(&AuditLog {
                id: ulid::Ulid::new().to_string(),
                user_id: Some(token.user_id),
                action: "logout".to_string(),
                ip_address: None,
                details: None,
                created_at: now,
            })
            .ok();
    }

    Ok(StatusCode::OK)
}

async fn setup_2fa_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();

    // Get first user for setup (JWT claims are validated by middleware already)
    let user = auth_db
        .list_users()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .next()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Generate TOTP secret
    let secret = totp_rs::Secret::generate_secret();
    let secret_encoded = secret.to_encoded().to_string();
    let secret_bytes = secret
        .to_bytes()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build TOTP
    let totp = TOTP {
        algorithm: TOTPAlgorithm::SHA1,
        digits: 6,
        skew: 1,
        step: 30,
        secret: secret_bytes,
        issuer: Some("Xavier".to_string()),
        account_name: user.email.clone(),
    };

    // Generate otpauth URL
    let otpauth_url = totp.get_url();

    // Generate QR code as Unicode (no need for SVG render feature)
    let qr = QrCode::new(otpauth_url.as_bytes()).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let qr_unicode = qr
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .build();

    // Generate backup codes (10 codes)
    let mut backup_codes = Vec::new();
    let mut hashed_codes = Vec::new();
    for _ in 0..10 {
        use rand::Rng;
        let code: u32 = rand::thread_rng().gen_range(10000000..99999999);
        let code_str = code.to_string();
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(code_str.as_bytes());
            crate::crypto::hex_encode(hasher.finalize())
        };
        backup_codes.push(code_str);
        hashed_codes.push(hash);
    }

    // Store secret + backup codes in DB
    auth_db
        .update_totp_secret(&user.id, &secret_encoded)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    auth_db
        .update_backup_codes(
            &user.id,
            &serde_json::to_string(&hashed_codes).unwrap_or_default(),
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    auth_db
        .log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id),
            action: "2fa_setup_initiated".to_string(),
            ip_address: None,
            details: None,
            created_at: now,
        })
        .ok();

    Ok(Json(TwoFactorSetupResponse {
        qr_code: qr_unicode,
        secret: secret_encoded,
        backup_codes,
    }))
}

async fn verify_2fa_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    Json(payload): Json<TwoFactorVerifyRequest>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();

    // Get first user (JWT claims validated by middleware)
    let user = auth_db
        .list_users()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .next()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let secret_b32 = user.totp_secret.as_ref().ok_or(StatusCode::BAD_REQUEST)?;
    let secret_bytes = Secret::Encoded(secret_b32.clone())
        .to_bytes()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let totp = TOTP {
        algorithm: TOTPAlgorithm::SHA1,
        digits: 6,
        skew: 1,
        step: 30,
        secret: secret_bytes,
        issuer: Some("Xavier".to_string()),
        account_name: user.email.clone(),
    };

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 30;
    if !totp.check(&payload.code, time) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    auth_db
        .enable_totp(&user.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    auth_db
        .log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id),
            action: "2fa_enabled".to_string(),
            ip_address: None,
            details: None,
            created_at: now,
        })
        .ok();

    Ok(Json(serde_json::json!({"status": "2fa_enabled"})))
}

async fn recovery_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    Json(payload): Json<RecoveryRequest>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();

    let user = auth_db
        .get_user_by_email(&payload.email)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Verify seed phrase
    let seed_hash = user
        .recovery_seed_hash
        .as_ref()
        .ok_or(StatusCode::BAD_REQUEST)?;
    let input_hash = {
        let mut hasher = Sha256::new();
        hasher.update(payload.seed_phrase.as_bytes());
        crate::crypto::hex_encode(hasher.finalize())
    };

    if &input_hash != seed_hash {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Reset password
    let new_hash =
        hash_password(&payload.new_password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    auth_db
        .update_password(&user.id, &new_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Disable 2FA
    auth_db
        .disable_totp(&user.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    auth_db
        .log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(user.id),
            action: "recovery_completed".to_string(),
            ip_address: None,
            details: None,
            created_at: now,
        })
        .ok();

    Ok(Json(serde_json::json!({"status": "recovery_completed"})))
}

async fn check_users_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
) -> Result<impl IntoResponse, StatusCode>
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => std::sync::Arc::new(parking_lot::Mutex::new(
            AuthDb::new(std::path::Path::new(&format!("{}/.xavier/auth.db", base_path)))
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        )),
    };
    let auth_db = auth_db_lock.lock();
    let count = auth_db
        .count_users()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "has_users": count > 0,
        "count": count,
    })))
}

async fn status_handler(req: Request) -> Result<impl IntoResponse, StatusCode> {
    // This route should be protected by middleware
    let claims = req
        .extensions()
        .get::<crate::auth2::jwt::Claims>()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    Ok(Json(serde_json::json!({
        "status": "authenticated",
        "user_id": claims.sub,
        "email": claims.email,
        "role": claims.role,
    })))
}
