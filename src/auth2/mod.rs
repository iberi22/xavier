pub mod password;
pub mod jwt;
pub mod db;
pub mod refresh;
pub mod middleware;

use sha2::Digest;
use axum::{
    routing::{get, post},
    Json, Router, extract::{State, Request},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
// use crate::cli::server::CliState;
use crate::auth2::db::{AuthDb, User, AuditLog};
use crate::auth2::password::{hash_password, verify_password};
use crate::auth2::jwt::JwtManager;
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

pub fn auth_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static
{
    Router::new()
        .route("/register", post(register_handler::<S>))
        .route("/login", post(login_handler::<S>))
        .route("/refresh", post(refresh_handler::<S>))
        .route("/logout", post(logout_handler::<S>))
        .route("/status", get(status_handler))
}

async fn register_handler<S>(
    State(_state): State<S>,
    Json(payload): Json<RegisterRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let auth_db = AuthDb::new(std::path::Path::new("auth.db")).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check if user exists
    if auth_db.get_user_by_email(&payload.email).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?.is_some() {
        return Err(StatusCode::CONFLICT);
    }

    let password_hash = hash_password(&payload.password).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

    let user = User {
        id: ulid::Ulid::new().to_string(),
        email: payload.email,
        password_hash,
        name: payload.name,
        role: "user".to_string(),
        totp_secret: None,
        totp_enabled: false,
        recovery_seed_hash: None,
        backup_codes: None,
        created_at: now,
        updated_at: now,
    };

    auth_db.create_user(&user).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    auth_db.log_audit(&AuditLog {
        id: ulid::Ulid::new().to_string(),
        user_id: Some(user.id),
        action: "register".to_string(),
        ip_address: None, // Should get from request
        details: None,
        created_at: now,
    }).ok();

    Ok(StatusCode::CREATED)
}

async fn login_handler<S>(
    State(_state): State<S>,
    Json(payload): Json<LoginRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let auth_db = AuthDb::new(std::path::Path::new("auth.db")).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let user = auth_db.get_user_by_email(&payload.email).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !verify_password(&payload.password, &user.password_hash).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Prepare TOTP check if enabled (placeholder for now)
    if user.totp_enabled {
        let code = payload.totp_code.ok_or(StatusCode::UNAUTHORIZED)?;
        // Verify TOTP...
    }

    let jwt_manager = JwtManager::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let access_token = jwt_manager.create_token(&user.id, &user.email, &user.role).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let refresh_manager = RefreshTokenManager::new(&auth_db);
    let refresh_token = refresh_manager.generate_token(&user.id, None).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    auth_db.log_audit(&AuditLog {
        id: ulid::Ulid::new().to_string(),
        user_id: Some(user.id),
        action: "login".to_string(),
        ip_address: None,
        details: None,
        created_at: now,
    }).ok();

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
    }))
}

async fn refresh_handler<S>(
    State(_state): State<S>,
    Json(payload): Json<RefreshRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let auth_db = AuthDb::new(std::path::Path::new("auth.db")).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let refresh_manager = RefreshTokenManager::new(&auth_db);

    let (new_refresh_token, user_id) = refresh_manager.rotate_token(&payload.refresh_token, None)
        .map_err(|e| {
            if e.to_string().contains("Potential theft detected") {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            }
        })?;

    let user = auth_db.get_user_by_id(&user_id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let jwt_manager = JwtManager::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let access_token = jwt_manager.create_token(&user.id, &user.email, &user.role).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token: new_refresh_token,
    }))
}

async fn logout_handler<S>(
    State(_state): State<S>,
    Json(payload): Json<RefreshRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let auth_db = AuthDb::new(std::path::Path::new("auth.db")).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let hash = {
        let mut hasher = sha2::Sha256::new();
        hasher.update(payload.refresh_token.as_bytes());
        crate::crypto::hex_encode(hasher.finalize())
    };

    if let Some(token) = auth_db.get_refresh_token_by_hash(&hash).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        auth_db.revoke_refresh_token(&token.id).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        auth_db.log_audit(&AuditLog {
            id: ulid::Ulid::new().to_string(),
            user_id: Some(token.user_id),
            action: "logout".to_string(),
            ip_address: None,
            details: None,
            created_at: now,
        }).ok();
    }

    Ok(StatusCode::OK)
}

async fn status_handler(
    req: Request,
) -> Result<impl IntoResponse, StatusCode> {
    // This route should be protected by middleware
    let claims = req.extensions().get::<crate::auth2::jwt::Claims>()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    Ok(Json(serde_json::json!({
        "status": "authenticated",
        "user_id": claims.sub,
        "email": claims.email,
        "role": claims.role,
    })))
}
