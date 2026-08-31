//! Clearance HTTP Middleware for Xavier API
//!
//! Provides the `clearance_middleware` function that reads clearance levels from
//! the `X-Clearance` HTTP header or JWT claims in request extensions. Constructs a
//! `ClearanceEnforcer` and inserts it into request extensions for downstream handlers.

use crate::security::auth::Claims;
use crate::security::clearance::{can_access, role_clearance, ClearanceEnforcer, ClearanceLevel};
use axum::{
    body::Body,
    http::{HeaderMap, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Header name used to supply requester clearance level directly.
pub const X_CLEARANCE_HEADER: &str = "x-clearance";

/// Header name used to specify the minimum clearance level required for a route.
pub const X_REQUIRED_CLEARANCE_HEADER: &str = "x-required-clearance";

/// Resolves the requester's `ClearanceLevel` from request headers or authentication claims.
pub fn resolve_requester_clearance(headers: &HeaderMap, claims: Option<&Claims>) -> ClearanceLevel {
    // 1. Try explicit X-Clearance header
    if let Some(val) = headers.get(X_CLEARANCE_HEADER) {
        if let Ok(s) = val.to_str() {
            return ClearanceLevel::from(s);
        }
    }

    // 2. Fallback to JWT role clearance
    if let Some(claims) = claims {
        return role_clearance(claims.role);
    }

    // 3. Default to Unclassified for unauthenticated/unspecified requests
    ClearanceLevel::Unclassified
}

/// Axum middleware that extracts requester clearance, inserts a `ClearanceEnforcer`
/// into request extensions, and enforces optional route-level clearance checks (`X-Required-Clearance`).
pub async fn clearance_middleware(mut req: Request<Body>, next: Next) -> Response {
    let claims = req.extensions().get::<Claims>().cloned();
    let requester_level = resolve_requester_clearance(req.headers(), claims.as_ref());
    let enforcer = ClearanceEnforcer::new(requester_level);

    // Enforce optional X-Required-Clearance check
    if let Some(required_hdr) = req.headers().get(X_REQUIRED_CLEARANCE_HEADER) {
        if let Ok(req_str) = required_hdr.to_str() {
            let required_level = ClearanceLevel::from(req_str);
            if !can_access(requester_level, required_level) {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "status": "error",
                        "message": format!(
                            "Forbidden: Insufficient clearance ({:?}) for required level ({:?})",
                            requester_level, required_level
                        )
                    })),
                )
                    .into_response();
            }
        }
    }

    req.extensions_mut().insert(requester_level);
    req.extensions_mut().insert(enforcer);

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::auth::UserRole;
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_clearance_middleware_header() {
        let app = Router::new()
            .route(
                "/data",
                get(|req: Request<Body>| async move {
                    let enforcer = req.extensions().get::<ClearanceEnforcer>().unwrap();
                    let redacted = enforcer.redact(ClearanceLevel::Secret, "super secret payload");
                    Json(json!({ "content": redacted }))
                }),
            )
            .layer(axum::middleware::from_fn(clearance_middleware));

        // Request with Confidential clearance -> Secret content is redacted
        let req = Request::builder()
            .uri("/data")
            .header(X_CLEARANCE_HEADER, "CONFIDENTIAL")
            .body(Body::empty())
            .unwrap();

        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Request with TopSecret clearance -> Secret content is accessible
        let req_ts = Request::builder()
            .uri("/data")
            .header(X_CLEARANCE_HEADER, "TOP_SECRET")
            .body(Body::empty())
            .unwrap();

        let resp_ts = app.oneshot(req_ts).await.unwrap();
        assert_eq!(resp_ts.status(), StatusCode::OK);
    }
}
