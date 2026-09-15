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
///
/// SECURITY: este header NO se confía por defecto. Solo se acepta si el operador
/// habilita explícitamente `XAVIER_CLEARANCE_TRUST_HEADER=1` (uso interno/loopback
/// o tests). El nivel real se deriva de las `Claims` autenticadas.
pub const X_CLEARANCE_HEADER: &str = "x-clearance";

/// Header name used to specify the minimum clearance level required for a route.
pub const X_REQUIRED_CLEARANCE_HEADER: &str = "x-required-clearance";

/// Env flag that re-enables trusting the `X-Clearance` header (default: OFF).
pub const TRUST_HEADER_ENV: &str = "XAVIER_CLEARANCE_TRUST_HEADER";

/// True when the operator opted in to trusting the `X-Clearance` header.
pub fn trusts_clearance_header() -> bool {
    matches!(
        std::env::var(TRUST_HEADER_ENV).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes")
    )
}

/// Clearance derived from the authenticated identity (JWT/lease/session/API token).
///
/// `Claims` are inserted by the auth middleware for every authenticated path, so
/// this is the authoritative source. No claims ⇒ `Unclassified`.
pub fn clearance_from_claims(claims: Option<&Claims>) -> ClearanceLevel {
    claims.map_or(ClearanceLevel::Unclassified, |c| role_clearance(c.role))
}

/// Resolves the requester's `ClearanceLevel` from the authenticated claims.
/// The `X-Clearance` header is IGNORED unless `XAVIER_CLEARANCE_TRUST_HEADER=1`.
pub fn resolve_requester_clearance(headers: &HeaderMap, claims: Option<&Claims>) -> ClearanceLevel {
    // 1. Identity first (authoritative).
    if let Some(claims) = claims {
        return role_clearance(claims.role);
    }

    // 2. Header only under explicit operator opt-in (internal/loopback/tests).
    if trusts_clearance_header() {
        if let Some(val) = headers.get(X_CLEARANCE_HEADER) {
            if let Ok(s) = val.to_str() {
                return ClearanceLevel::from(s);
            }
        }
    }

    // 3. Default: sin identidad ni opt-in ⇒ sin clasificación.
    ClearanceLevel::Unclassified
}

/// Middleware: deriva el nivel de la identidad autenticada y lo publica en las
/// extensiones de la request para que los handlers lo consuman.
///
/// Va SIEMPRE después de `auth_middleware` (ver `cli/server.rs`).
pub async fn clearance_session_middleware(mut req: Request<Body>, next: Next) -> Response {
    let claims = req.extensions().get::<Claims>().cloned();
    let level = resolve_requester_clearance(req.headers(), claims.as_ref());
    req.extensions_mut().insert(level);
    req.extensions_mut().insert(ClearanceEnforcer::new(level));
    next.run(req).await
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

    // ── Fase 1: el nivel sale de la identidad, no del header ─────────────────

    #[test]
    fn test_claims_take_precedence_over_header() {
        let mut headers = HeaderMap::new();
        headers.insert(X_CLEARANCE_HEADER, "UNCLASSIFIED".parse().unwrap());

        let admin = Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            UserRole::Admin,
            chrono::Duration::hours(1),
        );
        assert_eq!(
            resolve_requester_clearance(&headers, Some(&admin)),
            ClearanceLevel::TopSecret
        );

        // Aunque el header pida TOP_SECRET, el rol Readonly manda (Internal).
        headers.insert(X_CLEARANCE_HEADER, "TOP_SECRET".parse().unwrap());
        let readonly = Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            UserRole::Readonly,
            chrono::Duration::hours(1),
        );
        assert_eq!(
            resolve_requester_clearance(&headers, Some(&readonly)),
            ClearanceLevel::Internal
        );
    }

    #[test]
    fn test_header_ignored_without_optin() {
        if trusts_clearance_header() {
            eprintln!("skip: XAVIER_CLEARANCE_TRUST_HEADER activo en este entorno");
            return;
        }
        let mut headers = HeaderMap::new();
        headers.insert(X_CLEARANCE_HEADER, "TOP_SECRET".parse().unwrap());
        assert_eq!(
            resolve_requester_clearance(&headers, None),
            ClearanceLevel::Unclassified,
            "sin identidad el header no debe otorgar nivel"
        );
    }

    #[test]
    fn test_clearance_from_claims_mapping() {
        assert_eq!(clearance_from_claims(None), ClearanceLevel::Unclassified);
        let user = Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            UserRole::User,
            chrono::Duration::hours(1),
        );
        assert_eq!(
            clearance_from_claims(Some(&user)),
            ClearanceLevel::Confidential
        );
    }

    #[tokio::test]
    async fn test_session_middleware_publishes_identity_level() {
        let app = Router::new()
            .route(
                "/level",
                get(|req: Request<Body>| async move {
                    let level = *req.extensions().get::<ClearanceLevel>().unwrap();
                    if level == ClearanceLevel::Internal {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    }
                }),
            )
            .layer(axum::middleware::from_fn(clearance_session_middleware));

        // Readonly (⇒ Internal) con header TOP_SECRET: el header NO eleva el nivel.
        let mut req = Request::builder()
            .uri("/level")
            .header(X_CLEARANCE_HEADER, "TOP_SECRET")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(Claims::new(
            "u".into(),
            "u@swal.dev".into(),
            UserRole::Readonly,
            chrono::Duration::hours(1),
        ));
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::OK
        );

        // Sin identidad (solo header) ⇒ Unclassified ⇒ denegado.
        if !trusts_clearance_header() {
            let req = Request::builder()
                .uri("/level")
                .header(X_CLEARANCE_HEADER, "INTERNAL")
                .body(Body::empty())
                .unwrap();
            assert_eq!(
                app.oneshot(req).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
    }
}
