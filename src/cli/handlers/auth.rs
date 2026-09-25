//! Authentication API Handlers for Xavier
//!
//! This module has two unrelated groups of handlers:
//!
//! 1. **Deprecated legacy `/v1/auth/*` user-login endpoints** (`deprecated_v1_*_handler`,
//!    below). These used to be documented (openapi.yaml, docs/api/README.md, the Postman
//!    collection) as Xavier's canonical login API, but they were backed by
//!    `security::auth_store::AuthStore` — a user store with **zero production callers**:
//!    nothing in the codebase (no CLI command, no other route) ever wrote a user into it.
//!    Every credential check against it therefore failed forever, on every fresh install
//!    (see <https://github.com/iberi22/xavier/issues/2545>). The real, working auth lives at
//!    `/auth/*` (`xavier::auth2`, backed by `.xavier/auth.db`), populated by
//!    `xavier users create` / `xavier users totp-enroll`. These handlers keep the old paths
//!    mounted only so stale clients/docs get a clear, machine-readable pointer to the
//!    replacement instead of a bare 404.
//!
//! 2. **`/v1/auth/sessions*` root-token session handlers** (`list_sessions_handler`,
//!    `revoke_session_handler`), which are unrelated to user login: they manage sessions
//!    created from the root `XAVIER_TOKEN` and still use `AuthStore::get_active_sessions` /
//!    `AuthStore::revoke_user_session`. They are out of scope for the #2545 cleanup and are
//!    left untouched.

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use axum::{
    extract::{Path, State},
    http::{HeaderValue, StatusCode},
    response::Response,
};

/// Shared JSON body for every deprecated `/v1/auth/*` response: always carries the
/// replacement path so a stale client (or a human reading `curl -v` output) knows exactly
/// where to go next, regardless of whether the status code is a redirect or a hard stop.
fn deprecation_body(new_path: &str, detail: &str) -> serde_json::Value {
    serde_json::json!({
        "error": "endpoint_deprecated",
        "message": detail,
        "new_path": new_path,
        "docs": "https://github.com/iberi22/xavier/blob/main/docs/api/README.md",
    })
}

/// `308 Permanent Redirect` for a `/v1/auth/*` route whose `/auth/*` replacement accepts an
/// equivalent (or superset) request body, so a redirect-following client can retry the exact
/// same call against the new path and get a working response.
fn moved_permanently(new_path: &'static str, detail: &str) -> Response {
    let mut res = json_response(
        StatusCode::PERMANENT_REDIRECT,
        deprecation_body(new_path, detail),
    );
    res.headers_mut().insert(
        axum::http::header::LOCATION,
        HeaderValue::from_static(new_path),
    );
    res
}

/// `410 Gone` for a `/v1/auth/*` route with no request-compatible successor at a single path
/// (the functionality moved but the contract changed, e.g. merged into another call).
fn gone(new_path: &str, detail: &str) -> Response {
    json_response(StatusCode::GONE, deprecation_body(new_path, detail))
}

/// `POST /v1/auth/login` (deprecated) -> `POST /auth/login`.
///
/// Same `{email, password}` shape; the new endpoint additionally accepts an optional
/// `totp_code` field to complete 2FA in the same call (see `deprecated_v1_totp_verify_handler`).
pub async fn deprecated_v1_login_handler() -> Response {
    moved_permanently(
        "/auth/login",
        "POST /v1/auth/login is deprecated and never worked (dead AuthStore, see \
         github.com/iberi22/xavier/issues/2545). Use POST /auth/login instead. If the account \
         has 2FA enabled, include a totp_code field in the same request body — there is no \
         separate verify step.",
    )
}

/// `POST /v1/auth/register` (deprecated; this path was never actually mounted, only implied
/// by docs) -> `POST /auth/register`.
pub async fn deprecated_v1_register_handler() -> Response {
    moved_permanently(
        "/auth/register",
        "POST /v1/auth/register is deprecated. Use POST /auth/register with \
         {email, password, name}.",
    )
}

/// `POST /v1/auth/refresh` (deprecated) -> `POST /auth/refresh`.
///
/// Same `{refresh_token}` request shape.
pub async fn deprecated_v1_refresh_handler() -> Response {
    moved_permanently(
        "/auth/refresh",
        "POST /v1/auth/refresh is deprecated. Use POST /auth/refresh with the same \
         {refresh_token} body.",
    )
}

/// `POST /v1/auth/logout` (deprecated; this path was never actually mounted, only implied by
/// docs) -> `POST /auth/logout`.
pub async fn deprecated_v1_logout_handler() -> Response {
    moved_permanently(
        "/auth/logout",
        "POST /v1/auth/logout is deprecated. Use POST /auth/logout with the same \
         {refresh_token} body.",
    )
}

/// `POST /v1/auth/recover` (deprecated) -> `POST /auth/recovery`.
///
/// Same `{email, seed_phrase, new_password}` request shape.
pub async fn deprecated_v1_recover_handler() -> Response {
    moved_permanently(
        "/auth/recovery",
        "POST /v1/auth/recover is deprecated. Use POST /auth/recovery with the same \
         {email, seed_phrase, new_password} body.",
    )
}

/// `POST /v1/auth/totp/verify` (deprecated) — no request-compatible successor: TOTP
/// verification is now folded into `POST /auth/login` itself.
pub async fn deprecated_v1_totp_verify_handler() -> Response {
    gone(
        "/auth/login",
        "POST /v1/auth/totp/verify is gone; it has no standalone successor. The working flow \
         verifies TOTP inline: POST /auth/login with a totp_code field in the same request. To \
         enroll or confirm a new TOTP device, authenticate first and use POST /auth/2fa/setup \
         + POST /auth/2fa/verify.",
    )
}

/// `POST /v1/auth/totp/setup` (deprecated) — this path was an unmounted stub that always
/// returned `501 Not Implemented` and was never reachable from any route.
pub async fn deprecated_v1_totp_setup_handler() -> Response {
    gone(
        "/auth/2fa/setup",
        "POST /v1/auth/totp/setup is gone; it was never implemented (permanently returned 501 \
         Not Implemented) and was not reachable from any route. Use POST /auth/2fa/setup \
         (requires an authenticated session) or `xavier users totp-enroll --email <email>` from \
         the CLI.",
    )
}

/// List sessions handler.
pub async fn list_sessions_handler(
    State(state): State<CliState>,
    axum::Extension(session): axum::Extension<crate::cli::http_setup::SessionInfo>,
) -> Response {
    let user_id = match &session.user_id {
        Some(uid) => uid,
        None => {
            return json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "User not authenticated"}),
            )
        }
    };

    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Auth store not initialized"}),
            )
        }
    };

    match auth_store.get_active_sessions(user_id) {
        Ok(sessions) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "sessions": sessions
            }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": e.to_string()}),
        ),
    }
}

/// Revoke session handler.
pub async fn revoke_session_handler(
    State(state): State<CliState>,
    Path(token_id): Path<String>,
    axum::Extension(session): axum::Extension<crate::cli::http_setup::SessionInfo>,
) -> Response {
    let user_id = match &session.user_id {
        Some(uid) => uid,
        None => {
            return json_response(
                StatusCode::UNAUTHORIZED,
                serde_json::json!({"error": "User not authenticated"}),
            )
        }
    };

    let auth_store = match state.auth_store() {
        Some(s) => s,
        None => {
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({"error": "Auth store not initialized"}),
            )
        }
    };

    match auth_store.revoke_user_session(user_id, &token_id) {
        Ok(_) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "message": "Session revoked"
            }),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({"error": e.to_string()}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_json(res: Response) -> serde_json::Value {
        let bytes = to_bytes(res.into_body(), 8192).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn deprecated_login_redirects_permanently_to_auth_login() {
        let res = deprecated_v1_login_handler().await;
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            res.headers().get(axum::http::header::LOCATION).unwrap(),
            "/auth/login"
        );
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/login");
        assert_eq!(json["error"], "endpoint_deprecated");
    }

    #[tokio::test]
    async fn deprecated_register_redirects_permanently_to_auth_register() {
        let res = deprecated_v1_register_handler().await;
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/register");
    }

    #[tokio::test]
    async fn deprecated_refresh_redirects_permanently_to_auth_refresh() {
        let res = deprecated_v1_refresh_handler().await;
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/refresh");
    }

    #[tokio::test]
    async fn deprecated_logout_redirects_permanently_to_auth_logout() {
        let res = deprecated_v1_logout_handler().await;
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/logout");
    }

    #[tokio::test]
    async fn deprecated_recover_redirects_permanently_to_auth_recovery() {
        let res = deprecated_v1_recover_handler().await;
        assert_eq!(res.status(), StatusCode::PERMANENT_REDIRECT);
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/recovery");
    }

    #[tokio::test]
    async fn deprecated_totp_verify_is_gone() {
        let res = deprecated_v1_totp_verify_handler().await;
        assert_eq!(res.status(), StatusCode::GONE);
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/login");
    }

    #[tokio::test]
    async fn deprecated_totp_setup_is_gone() {
        let res = deprecated_v1_totp_setup_handler().await;
        assert_eq!(res.status(), StatusCode::GONE);
        let json = body_json(res).await;
        assert_eq!(json["new_path"], "/auth/2fa/setup");
    }
}
