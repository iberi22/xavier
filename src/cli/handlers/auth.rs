//! Authentication API Handlers for Xavier

use axum::{extract::{State, ConnectInfo}, http::{StatusCode, HeaderMap}, response::Response, Json};
use serde::{Deserialize, Serialize};
use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::security::auth::{User, UserRole, generate_jwt, validate_jwt, TotpProvider};
use xavier::crypto::password;
use xavier::security::recovery::RecoverySystem;
use chrono::{Utc, Duration};

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
}

#[derive(Deserialize)]
pub struct TotpVerifyRequest {
    pub email: String,
    pub code: String,
}

#[derive(Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Deserialize)]
pub struct RecoverRequest {
    pub email: String,
    pub seed_phrase: String,
    pub new_password: String,
}

pub async fn register_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    if auth_store.get_user_by_email(&payload.email).unwrap_or(None).is_some() {
        return json_response(StatusCode::CONFLICT, serde_json::json!({"error": "User already exists"}));
    }

    let password_hash = match password::hash(&payload.password, 0) {
        Ok(h) => h,
        Err(e) => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()})),
    };

    let user = User::new(payload.email.clone(), payload.name, UserRole::User);
    let seed_phrase = RecoverySystem::generate_phrase();

    if let Err(e) = auth_store.create_user(&user, &password_hash) {
        return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()}));
    }

    if let Err(e) = auth_store.set_recovery_phrase(&user.id, &seed_phrase) {
        return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()}));
    }

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let _ = auth_store.log_event(Some(&user.id), "register", ip.as_deref(), ua.as_deref(), None);

    json_response(StatusCode::CREATED, serde_json::json!({
        "status": "ok",
        "user_id": user.id,
        "seed_phrase": seed_phrase
    }))
}

pub async fn login_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let (user, hash) = match auth_store.get_user_by_email(&payload.email).unwrap_or(None) {
        Some(u) => u,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid credentials"})),
    };

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    if !password::verify(&payload.password, &hash).unwrap_or(false) {
        let _ = auth_store.log_event(Some(&user.id), "login_failed", ip.as_deref(), ua.as_deref(), None);
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid credentials"}));
    }

    // Check if TOTP is enabled
    let totp_secret = auth_store.get_totp_secret(&user.id).unwrap_or(None);
    if totp_secret.is_some() {
        return json_response(StatusCode::ACCEPTED, serde_json::json!({
            "status": "mfa_required",
            "email": user.email
        }));
    }

    issue_tokens(&state, &user, ip.as_deref(), ua.as_deref()).await
}

pub async fn totp_verify_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<TotpVerifyRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let (user, _) = match auth_store.get_user_by_email(&payload.email).unwrap_or(None) {
        Some(u) => u,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid user"})),
    };

    let secret = match auth_store.get_totp_secret(&user.id).unwrap_or(None) {
        Some(s) => s,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "TOTP not enabled"})),
    };

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    let totp = TotpProvider::new("Xavier");
    if !totp.verify_code(&secret, &payload.code) {
        let _ = auth_store.log_event(Some(&user.id), "totp_failed", ip.as_deref(), ua.as_deref(), None);
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid TOTP code"}));
    }

    issue_tokens(&state, &user, ip.as_deref(), ua.as_deref()).await
}

pub async fn refresh_handler(
    State(state): State<CliState>,
    Json(payload): Json<RefreshRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let user_id = match auth_store.verify_refresh_token(&payload.refresh_token).unwrap_or(None) {
        Some(id) => id,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid refresh token"})),
    };

    // Revoke old token
    let _ = auth_store.revoke_refresh_token(&payload.refresh_token);

    let user = auth_store.get_user_by_id(&user_id).unwrap_or(None);
    match user {
        Some(u) => issue_tokens(&state, &u, None, None).await,
        None => json_response(StatusCode::NOT_FOUND, serde_json::json!({"error": "User not found"}))
    }
}

pub async fn recover_handler(
    State(state): State<CliState>,
    headers: HeaderMap,
    Json(payload): Json<RecoverRequest>,
) -> Response {
    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": "Auth store not initialized"})),
    };

    let (user, _) = match auth_store.get_user_by_email(&payload.email).unwrap_or(None) {
        Some(u) => u,
        None => return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid user"})),
    };

    let stored_phrase = match auth_store.get_recovery_phrase(&user.id).unwrap_or(None) {
        Some(p) => p,
        None => return json_response(StatusCode::BAD_REQUEST, serde_json::json!({"error": "Recovery not enabled"})),
    };

    let ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let ua = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    if stored_phrase != payload.seed_phrase {
        let _ = auth_store.log_event(Some(&user.id), "recovery_failed", ip.as_deref(), ua.as_deref(), None);
        return json_response(StatusCode::UNAUTHORIZED, serde_json::json!({"error": "Invalid seed phrase"}));
    }

    let new_hash = password::hash(&payload.new_password, 0).unwrap();
    auth_store.update_password(&user.id, &new_hash).unwrap();

    let _ = auth_store.log_event(Some(&user.id), "recovery_success", ip.as_deref(), ua.as_deref(), None);

    json_response(StatusCode::OK, serde_json::json!({"status": "ok"}))
}

pub async fn totp_setup_handler(
    State(state): State<CliState>,
) -> Response {
    // This should be protected by JWT middleware
    // For now we'll assume we get the user from the state or a placeholder
    // In a real impl, we'd extract it from the token
    json_response(StatusCode::NOT_IMPLEMENTED, serde_json::json!({"error": "Not implemented"}))
}

async fn issue_tokens(state: &CliState, user: &User, ip: Option<&str>, ua: Option<&str>) -> Response {
    let auth_store = state.auth_store().unwrap();
    let secret = std::env::var("XAVIER_JWT_SECRET").unwrap_or_else(|_| "default_secret_change_me".to_string());

    let token: String = match generate_jwt(user, secret.as_bytes()) {
        Ok(t) => t,
        Err(e) => return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()})),
    };

    let refresh_token = ulid::Ulid::new().to_string();
    let expires_at = (Utc::now() + Duration::days(7)).timestamp();

    if let Err(e) = auth_store.save_refresh_token(&refresh_token, &user.id, expires_at) {
        return json_response(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({"error": e.to_string()}));
    }

    let _ = auth_store.log_event(Some(&user.id), "login_success", ip, ua, None);

    json_response(StatusCode::OK, serde_json::json!({
        "status": "ok",
        "access_token": token,
        "refresh_token": refresh_token,
        "user": user
    }))
}
