//! Token management handlers for API tokens.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
    Json,
};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::cli::types::CreateTokenPayload;
use xavier::security::tokens::TokenStore;

pub async fn list_tokens_handler(State(_state): State<CliState>) -> Response {
    let store = TokenStore::new();
    match store.list_tokens().await {
        Ok(tokens) => json_response(
            StatusCode::OK,
            serde_json::json!({ "status": "ok", "tokens": tokens }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}

pub async fn create_token_handler(
    State(_state): State<CliState>,
    Json(payload): Json<CreateTokenPayload>,
) -> Response {
    let store = TokenStore::new();
    match store
        .create_token(payload.name, payload.scopes, payload.expires_at)
        .await
    {
        Ok((plaintext, metadata)) => json_response(
            StatusCode::CREATED,
            serde_json::json!({
                "status": "ok",
                "token": plaintext,
                "metadata": metadata
            }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}

pub async fn revoke_token_handler(
    State(_state): State<CliState>,
    Path(id): Path<String>,
) -> Response {
    let store = TokenStore::new();
    match store.revoke_token(&id).await {
        Ok(_) => json_response(StatusCode::OK, serde_json::json!({ "status": "ok" })),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}

pub async fn rotate_token_handler(
    State(_state): State<CliState>,
    Path(id): Path<String>,
) -> Response {
    let store = TokenStore::new();

    // 1. Get existing metadata to replicate it (simplified, just name/scopes/expiry)
    let tokens = match store.list_tokens().await {
        Ok(t) => t,
        Err(e) => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({ "status": "error", "message": e.to_string() }),
            )
        }
    };

    let existing = match tokens.into_iter().find(|t| t.id == id) {
        Some(t) => t,
        None => {
            return json_response(
                StatusCode::NOT_FOUND,
                serde_json::json!({ "status": "error", "message": "Token not found" }),
            )
        }
    };

    // 2. Revoke old
    if let Err(e) = store.revoke_token(&id).await {
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        );
    }

    // 3. Create new
    match store
        .create_token(existing.name, existing.scopes, existing.expires_at)
        .await
    {
        Ok((plaintext, metadata)) => json_response(
            StatusCode::CREATED,
            serde_json::json!({
                "status": "ok",
                "token": plaintext,
                "metadata": metadata
            }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        ),
    }
}
