//! Authentication and RBAC Middleware for Xavier API
//!
//! Provides the `require_permission` middleware function that checks
//! JWT claims or session roles in request extensions against specific
//! `Permission` checks from `crate::security::auth`.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::future::Future;
use std::pin::Pin;

use crate::security::auth::{Claims, UserRole};

/// Creates an Axum middleware function that requires a specific permission on `UserRole`.
///
/// Returns 403 Forbidden if claims are missing or if the check evaluates to `false`.
pub fn require_permission(
    check: fn(&UserRole) -> bool,
) -> impl Fn(Request<Body>, Next) -> Pin<Box<dyn Future<Output = Response> + Send>> + Clone {
    move |req: Request<Body>, next: Next| {
        Box::pin(async move {
            let claims = req.extensions().get::<Claims>();
            match claims {
                Some(claims) => {
                    if check(&claims.role) {
                        next.run(req).await
                    } else {
                        (
                            StatusCode::FORBIDDEN,
                            Json(json!({
                                "status": "error",
                                "message": "Forbidden: Insufficient permissions"
                            })),
                        )
                            .into_response()
                    }
                }
                None => (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "status": "error",
                        "message": "Forbidden: Missing authentication claims"
                    })),
                )
                    .into_response(),
            }
        })
    }
}
