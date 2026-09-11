//! Authentication and authorization module.
//!
//! Provides JWT-based authentication, password hashing/verification,
//! refresh token rotation, and RBAC middleware for securing API endpoints.
//! Includes database-backed session and user stores.

pub mod db;
pub mod jwt;
pub mod middleware;
pub mod oauth;
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
use totp_rs::{Algorithm as TOTPAlgorithm, Builder, Secret, Totp};
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
        // Vincular un proveedor a la cuenta AUTENTICADA (requiere sesion abierta).
        .route("/oauth/link", post(oauth_link_handler::<S>))
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
        // OAuth: inicio del flujo y callback. Se montan bajo /auth (igual que register/login).
        .route("/oauth/{provider}", get(oauth_start_handler))
        .route(
            "/oauth/{provider}/callback",
            get(oauth_callback_handler::<S>),
        )
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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
            let secret = Secret::try_from_base32(secret.as_str())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let totp = Builder::new()
                .with_algorithm(TOTPAlgorithm::SHA1)
                .with_digits(6)
                .with_skew(1)
                .with_step_duration(30)
                .with_secret(secret)
                .with_account_name(user.email.clone())
                .with_issuer(Some("Xavier".to_string()))
                .build()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                / 30;
            if totp.check(&code, time).is_none() {
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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

    // Generate TOTP secret (gen_secret auto-generates inside build)
    let totp = Builder::new()
        .with_algorithm(TOTPAlgorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_account_name(user.email.clone())
        .with_issuer(Some("Xavier".to_string()))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let secret_encoded = totp.secret().to_base32();

    // Generate otpauth URL
    let otpauth_url = totp
        .to_url()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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
    let secret = Secret::try_from_base32(secret_b32.as_str())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let totp = Builder::new()
        .with_algorithm(TOTPAlgorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(secret)
        .with_account_name(user.email.clone())
        .with_issuer(Some("Xavier".to_string()))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 30;
    if totp.check(&payload.code, time).is_none() {
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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
            AuthDb::new(std::path::Path::new(&format!(
                "{}/.xavier/auth.db",
                base_path
            )))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
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

// ── OAuth 2.0 (Google, GitHub) ──────────────────────────────────────────────────
//
// Modelo de seguridad (ver `auth2::oauth` para el detalle):
//   * el vinculo con el proveedor es por `subject`, nunca por email;
//   * un email que ya existe NO se fusiona automaticamente: exige vincular con sesion abierta;
//   * `state` firmado con caducidad y PKCE S256;
//   * el email debe venir verificado por el proveedor para dar de alta una cuenta nueva.

/// Parametros que devuelve el proveedor al callback.
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

fn json_err(status: StatusCode, body: serde_json::Value) -> axum::response::Response {
    (status, Json(body)).into_response()
}

/// `GET /auth/oauth/{provider}` — inicia el flujo y redirige al proveedor.
///
/// Si el proveedor no esta configurado responde **501 con las variables exactas que faltan**:
/// es preferible un error explicito a un flujo que falla a mitad del navegador.
async fn oauth_start_handler(
    axum::extract::Path(provider_slug): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::Redirect;

    let Some(provider) = oauth::Provider::from_slug(&provider_slug) else {
        return json_err(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "proveedor no soportado", "soportados": ["google", "github"] }),
        );
    };

    let cfg = oauth::OAuthConfig::from_env();
    if cfg.provider(provider).is_none() {
        let pref = format!("XAVIER_OAUTH_{}_", provider.slug().to_ascii_uppercase());
        return json_err(
            StatusCode::NOT_IMPLEMENTED,
            serde_json::json!({
                "error": "proveedor no configurado",
                "faltan": [format!("{pref}CLIENT_ID"), format!("{pref}CLIENT_SECRET")],
                "redirect_uri_a_registrar": cfg.redirect_uri(provider),
            }),
        );
    }

    let verifier = oauth::new_code_verifier();
    let challenge = oauth::code_challenge_s256(&verifier);
    let state = oauth::new_state(&cfg.state_secret, provider, &verifier, 600);

    match oauth::authorize_url(&cfg, provider, &state, &challenge) {
        Some(url) => Redirect::temporary(&url).into_response(),
        None => json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "no se pudo construir la URL de autorizacion" }),
        ),
    }
}

/// `GET /auth/oauth/{provider}/callback` — canjea el codigo, resuelve la identidad y emite sesion.
async fn oauth_callback_handler<S>(
    State(state): State<S>,
    axum::extract::Path(provider_slug): axum::extract::Path<String>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    axum::extract::Query(q): axum::extract::Query<OAuthCallbackQuery>,
) -> axum::response::Response
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    // 0. Error devuelto por el proveedor (usuario cancelo, etc.)
    if let Some(err) = q.error {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": err, "detalle": q.error_description }),
        );
    }

    let Some(provider) = oauth::Provider::from_slug(&provider_slug) else {
        return json_err(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "proveedor no soportado" }),
        );
    };

    let cfg = oauth::OAuthConfig::from_env();
    if cfg.provider(provider).is_none() {
        return json_err(
            StatusCode::NOT_IMPLEMENTED,
            serde_json::json!({ "error": "proveedor no configurado" }),
        );
    }

    // 1. `state`: firma, caducidad y coherencia con el proveedor de la ruta
    let Some(state_param) = q.state else {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "falta state" }),
        );
    };
    let Some((state_provider, verifier)) = oauth::verify_state(&cfg.state_secret, &state_param)
    else {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "state invalido o caducado" }),
        );
    };
    if state_provider != provider {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "state de otro proveedor" }),
        );
    }

    // 2. Codigo -> identidad
    let Some(code) = q.code else {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "falta code" }),
        );
    };
    let identidad = match oauth::exchange_code(&cfg, provider, &code, &verifier).await {
        Ok(i) => i,
        Err(e) => {
            // El detalle se registra, no se devuelve: puede contener el cuerpo del proveedor.
            tracing::warn!(error = %e, provider = provider.slug(), "oauth: fallo el canje del codigo");
            return json_err(
                StatusCode::BAD_GATEWAY,
                serde_json::json!({ "error": "no se pudo validar el codigo con el proveedor" }),
            );
        }
    };

    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => {
            let path = format!("{}/.xavier/auth.db", base_path);
            match AuthDb::new(std::path::Path::new(&path)) {
                Ok(db) => std::sync::Arc::new(parking_lot::Mutex::new(db)),
                Err(_) => {
                    return json_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        serde_json::json!({ "error": "auth db no disponible" }),
                    )
                }
            }
        }
    };
    let auth_db = auth_db_lock.lock();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // 3. Ya vinculada -> login
    let vinculado = match auth_db.get_user_by_oauth(provider.slug(), &identidad.subject) {
        Ok(v) => v,
        Err(_) => {
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "no se pudo consultar la vinculacion" }),
            )
        }
    };

    let user = match vinculado {
        Some(u) => u,
        None => {
            // 4. Alta nueva: exige email VERIFICADO por el proveedor
            let Some(email) = identidad.email.clone() else {
                return json_err(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({
                        "error": "el proveedor no entrego un correo",
                        "hint": "autoriza el permiso de email en el proveedor y reintenta"
                    }),
                );
            };
            if !identidad.email_verified {
                return json_err(
                    StatusCode::FORBIDDEN,
                    serde_json::json!({
                        "error": "el proveedor no marca ese correo como verificado",
                        "hint": "verifica tu correo en el proveedor y reintenta"
                    }),
                );
            }

            // 5. El email ya existe: NO se fusiona solo (evita apropiarse de una cuenta ajena)
            let ya_existe = auth_db
                .get_user_by_email(&email)
                .map(|u| u.is_some())
                .unwrap_or(false);
            if ya_existe {
                return json_err(
                    StatusCode::CONFLICT,
                    serde_json::json!({
                        "error": "cuenta_existente",
                        "hint": "inicia sesion con tu contrasena y vincula el proveedor desde /auth/oauth/link"
                    }),
                );
            }

            let nuevo = User {
                id: ulid::Ulid::new().to_string(),
                email: email.clone(),
                // Cuenta sin contrasena: marca inutilizable (nunca coincide con un hash argon2).
                password_hash: "!oauth-no-password".to_string(),
                name: email.clone(),
                role: "user".to_string(),
                totp_secret: None,
                totp_enabled: false,
                recovery_seed_hash: None,
                backup_codes: None,
                created_at: now,
                updated_at: now,
            };
            if auth_db.create_user(&nuevo).is_err() {
                return json_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    serde_json::json!({ "error": "no se pudo crear la cuenta" }),
                );
            }
            let _ = auth_db.mark_email_verified(&nuevo.id, now);
            let _ = auth_db.link_oauth_identity(
                provider.slug(),
                &identidad.subject,
                &nuevo.id,
                Some(&email),
            );
            let _ = auth_db.log_audit(&AuditLog {
                id: ulid::Ulid::new().to_string(),
                user_id: Some(nuevo.id.clone()),
                action: format!("register_oauth:{}", provider.slug()),
                ip_address: None,
                details: None,
                created_at: now,
            });
            nuevo
        }
    };

    // 6. Sesion (mismo contrato que el login local)
    let jwt_manager = match JwtManager::new() {
        Ok(j) => j,
        Err(_) => {
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "no se pudo emitir el token" }),
            )
        }
    };
    let access_token = match jwt_manager.create_token(&user.id, &user.email, &user.role) {
        Ok(t) => t,
        Err(_) => {
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "no se pudo emitir el token" }),
            )
        }
    };
    let refresh_token = RefreshTokenManager::new(&auth_db)
        .generate_token(&user.id, Some(format!("oauth:{}", provider.slug())))
        .unwrap_or_default();

    Json(serde_json::json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "requires_2fa": false,
        "user": UserResponse::from(user),
        "provider": provider.slug(),
    }))
    .into_response()
}

/// Cuerpo de `POST /auth/oauth/link`.
#[derive(Debug, Deserialize)]
pub struct OAuthLinkRequest {
    pub provider: String,
    pub code: String,
    pub state: String,
}

/// `POST /auth/oauth/link` — vincula un proveedor a la cuenta **autenticada**.
///
/// Existe porque un email que ya tiene cuenta **no** se fusiona automaticamente en el callback: el
/// usuario debe demostrar que controla la cuenta iniciando sesion y vinculando desde aqui.
///
/// El dueno de la vinculacion sale de `claims.sub` (el JWT que valido el middleware), NO de una
/// consulta a la base. Nota: el handler de 2FA existente toma `list_users().next()`, o sea el primer
/// usuario de la base, que con mas de una cuenta vincularia la identidad a quien no es; aqui no se
/// repite ese patron.
async fn oauth_link_handler<S>(
    State(state): State<S>,
    axum::Extension(base_path): axum::Extension<std::sync::Arc<String>>,
    axum::Extension(claims): axum::Extension<crate::auth2::jwt::Claims>,
    Json(payload): Json<OAuthLinkRequest>,
) -> axum::response::Response
where
    S: HasAuthDb + Clone + Send + Sync + 'static,
{
    let Some(provider) = oauth::Provider::from_slug(&payload.provider) else {
        return json_err(
            StatusCode::NOT_FOUND,
            serde_json::json!({ "error": "proveedor no soportado", "soportados": ["google", "github"] }),
        );
    };

    let cfg = oauth::OAuthConfig::from_env();
    if cfg.provider(provider).is_none() {
        return json_err(
            StatusCode::NOT_IMPLEMENTED,
            serde_json::json!({ "error": "proveedor no configurado" }),
        );
    }

    let Some((state_provider, verifier)) = oauth::verify_state(&cfg.state_secret, &payload.state)
    else {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "state invalido o caducado" }),
        );
    };
    if state_provider != provider {
        return json_err(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "error": "state de otro proveedor" }),
        );
    }

    let identidad = match oauth::exchange_code(&cfg, provider, &payload.code, &verifier).await {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, provider = provider.slug(), "oauth link: fallo el canje del codigo");
            return json_err(
                StatusCode::BAD_GATEWAY,
                serde_json::json!({ "error": "no se pudo validar el codigo con el proveedor" }),
            );
        }
    };

    let auth_db_lock = match state.auth_db() {
        Some(db) => db,
        None => {
            let path = format!("{}/.xavier/auth.db", base_path);
            match AuthDb::new(std::path::Path::new(&path)) {
                Ok(db) => std::sync::Arc::new(parking_lot::Mutex::new(db)),
                Err(_) => {
                    return json_err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        serde_json::json!({ "error": "auth db no disponible" }),
                    )
                }
            }
        }
    };
    let auth_db = auth_db_lock.lock();

    // La identidad no puede pertenecer ya a OTRA cuenta.
    match auth_db.get_user_by_oauth(provider.slug(), &identidad.subject) {
        Ok(Some(otro)) if otro.id != claims.sub => {
            return json_err(
                StatusCode::CONFLICT,
                serde_json::json!({
                    "error": "esa identidad ya esta vinculada a otra cuenta",
                }),
            )
        }
        Ok(_) => {}
        Err(_) => {
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "error": "no se pudo consultar la vinculacion" }),
            )
        }
    }

    if auth_db
        .link_oauth_identity(
            provider.slug(),
            &identidad.subject,
            &claims.sub,
            identidad.email.as_deref(),
        )
        .is_err()
    {
        return json_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": "no se pudo guardar la vinculacion" }),
        );
    }

    // Si el proveedor confirma el correo, la cuenta queda con el email verificado.
    if identidad.email_verified {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let _ = auth_db.mark_email_verified(&claims.sub, ts);
    }

    let _ = auth_db.log_audit(&AuditLog {
        id: ulid::Ulid::new().to_string(),
        user_id: Some(claims.sub.clone()),
        action: format!("link_oauth:{}", provider.slug()),
        ip_address: None,
        details: None,
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
    });

    Json(serde_json::json!({
        "linked": true,
        "provider": provider.slug(),
        "email_verified": identidad.email_verified,
    }))
    .into_response()
}
