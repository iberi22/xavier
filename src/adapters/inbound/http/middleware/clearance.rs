//! Clearance HTTP Middleware for Xavier API
//!
//! Provides the `clearance_middleware` function that reads clearance levels from
//! the `X-Clearance` HTTP header or JWT claims in request extensions. Constructs a
//! `ClearanceEnforcer` and inserts it into request extensions for downstream handlers.

use crate::security::auth::Claims;
use crate::security::clearance::{
    can_access, parse_requester_level, parse_required_level, role_clearance, ClearanceEnforcer,
    ClearanceLevel,
};
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
                return parse_requester_level(s);
            }
        }
    }

    // 3. Default: sin identidad ni opt-in ⇒ sin clasificación.
    ClearanceLevel::Unclassified
}

/// Unión del nivel base con el techo que otorgan los segmentos.
///
/// `ceiling_lookup` se inyecta para poder testear la política sin tocar el
/// registro real del workspace.
pub fn effective_clearance_with(
    base: ClearanceLevel,
    member_ids: &[&str],
    ceiling_lookup: impl Fn(&[&str]) -> Option<ClearanceLevel>,
) -> ClearanceLevel {
    match ceiling_lookup(member_ids) {
        Some(segment_level) => base.max(segment_level),
        None => base,
    }
}

/// Techo efectivo del solicitante: nivel del rol **∪** nivel de sus segmentos.
///
/// F3.1: la pertenencia a un segmento del laboratorio (con permiso de lectura)
/// eleva el techo del miembro. La elevación **exige identidad autenticada**: sin
/// `Claims` no hay membresía que evaluar, así que un anónimo nunca sube de nivel
/// (ni con el header bajo opt-in).
pub fn resolve_effective_clearance(headers: &HeaderMap, claims: Option<&Claims>) -> ClearanceLevel {
    let base = resolve_requester_clearance(headers, claims);
    match claims {
        Some(claims) => effective_clearance_with(
            base,
            &[claims.sub.as_str(), claims.email.as_str()],
            crate::security::groups::shared_member_ceiling,
        ),
        None => base,
    }
}

/// Identidad mínima del solicitante para auditoría (F3.2).
#[derive(Debug, Clone)]
pub struct RequesterIdentity {
    pub subject: String,
    pub role: String,
}

/// Exigencia de nivel por **configuración de ruta** (F3.2).
///
/// Devuelve `Some(respuesta 403)` cuando el solicitante no alcanza el nivel que la
/// ruta exige, y deja la denegación auditada. `None` = la ruta no exige nada.
pub fn enforce_route_policy(
    path: &str,
    requester_level: ClearanceLevel,
    claims: Option<&Claims>,
) -> Option<Response> {
    let required = crate::security::route_policy::required_for_path(path)?;
    if can_access(requester_level, required) {
        return None;
    }

    crate::security::clearance_audit::record_denied(claims, requester_level, path, "route_denied");
    tracing::warn!(
        route = path,
        requester = ?requester_level,
        required = ?required,
        "lectura denegada por política de ruta"
    );

    Some(
        (
            StatusCode::FORBIDDEN,
            Json(json!({
                "status": "error",
                "message": format!(
                    "Forbidden: route requires {:?} (requester has {:?})",
                    required, requester_level
                ),
            })),
        )
            .into_response(),
    )
}

/// Middleware: deriva el nivel de la identidad autenticada y lo publica en las
/// extensiones de la request para que los handlers lo consuman.
///
/// Va SIEMPRE después de `auth_middleware` (ver `cli/server.rs`).
pub async fn clearance_session_middleware(mut req: Request<Body>, next: Next) -> Response {
    let claims = req.extensions().get::<Claims>().cloned();
    let level = resolve_effective_clearance(req.headers(), claims.as_ref());

    if let Some(denied) = enforce_route_policy(req.uri().path(), level, claims.as_ref()) {
        return denied;
    }

    req.extensions_mut().insert(level);
    req.extensions_mut().insert(ClearanceEnforcer::new(level));

    let (subject, role) = crate::security::clearance_audit::subject_and_role(claims.as_ref());
    req.extensions_mut()
        .insert(RequesterIdentity { subject, role });

    next.run(req).await
}

/// Axum middleware that extracts requester clearance, inserts a `ClearanceEnforcer`
/// into request extensions, and enforces optional route-level clearance checks (`X-Required-Clearance`).
pub async fn clearance_middleware(mut req: Request<Body>, next: Next) -> Response {
    let claims = req.extensions().get::<Claims>().cloned();
    let requester_level = resolve_effective_clearance(req.headers(), claims.as_ref());
    let enforcer = ClearanceEnforcer::new(requester_level);

    // Enforce optional X-Required-Clearance check
    if let Some(required_hdr) = req.headers().get(X_REQUIRED_CLEARANCE_HEADER) {
        if let Ok(req_str) = required_hdr.to_str() {
            // Exigencia: valor desconocido restringe (fail-closed).
            let required_level = parse_required_level(req_str);
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

    // ── F3.1: techo efectivo = rol ∪ segmentos ──────────────────────────────

    #[test]
    fn test_effective_clearance_is_union_and_never_lowers() {
        // Miembro de un segmento Secret: sube desde su rol (Internal).
        assert_eq!(
            effective_clearance_with(ClearanceLevel::Internal, &["nodo-1"], |_| Some(
                ClearanceLevel::Secret
            )),
            ClearanceLevel::Secret
        );
        // Sin segmento: se queda con su rol.
        assert_eq!(
            effective_clearance_with(ClearanceLevel::Confidential, &["nodo-2"], |_| None),
            ClearanceLevel::Confidential
        );
        // El rol más alto manda: la membresía no degrada a nadie.
        assert_eq!(
            effective_clearance_with(ClearanceLevel::TopSecret, &["nodo-3"], |_| Some(
                ClearanceLevel::Internal
            )),
            ClearanceLevel::TopSecret
        );
        // Los identificadores llegan al lookup (sub y email).
        let seen: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        let _ = effective_clearance_with(ClearanceLevel::Internal, &["sub-1", "a@b.c"], |ids| {
            seen.borrow_mut()
                .extend(ids.iter().map(|id| id.to_string()));
            None
        });
        assert_eq!(
            *seen.borrow(),
            vec!["sub-1".to_string(), "a@b.c".to_string()]
        );
    }

    #[test]
    fn test_route_policy_denies_below_required() {
        let audit_file = tempfile::NamedTempFile::new().unwrap();
        std::env::set_var(
            crate::security::clearance_audit::CLEARANCE_AUDIT_ENV,
            audit_file.path().to_string_lossy().to_string(),
        );
        std::env::set_var(
            crate::security::route_policy::ROUTE_POLICY_ENV,
            r#"{"routes":[{"prefix":"/segments","required":"SECRET"}]}"#,
        );

        // Un usuario CONFIDENTIAL no alcanza SECRET ⇒ 403 y queda auditado.
        let denied = enforce_route_policy(
            "/segments/seg-research/plan",
            ClearanceLevel::Confidential,
            None,
        );
        assert_eq!(
            denied.map(|response| response.status()),
            Some(StatusCode::FORBIDDEN)
        );

        // SECRET sí pasa.
        assert!(
            enforce_route_policy("/segments/seg-research/plan", ClearanceLevel::Secret, None)
                .is_none()
        );

        // Fuera del prefijo configurado no se exige nada.
        assert!(
            enforce_route_policy("/memory/search", ClearanceLevel::Unclassified, None).is_none()
        );

        let raw = std::fs::read_to_string(audit_file.path()).unwrap();
        assert!(
            raw.contains("route_denied"),
            "la denegación queda auditada: {raw}"
        );

        std::env::remove_var(crate::security::route_policy::ROUTE_POLICY_ENV);
        std::env::remove_var(crate::security::clearance_audit::CLEARANCE_AUDIT_ENV);
    }

    #[test]
    fn test_effective_clearance_anonymous_never_lifts() {
        // Sin identidad no hay membresía que evaluar: ni siquiera con el header.
        let anon = resolve_effective_clearance(&HeaderMap::new(), None);
        assert_eq!(anon, ClearanceLevel::Unclassified);

        let readonly = Claims::new(
            "nodo-sin-segmento".into(),
            "nodo@swal.dev".into(),
            UserRole::Readonly,
            chrono::Duration::hours(1),
        );
        assert_eq!(
            resolve_effective_clearance(&HeaderMap::new(), Some(&readonly)),
            ClearanceLevel::Internal,
            "un usuario fuera de segmentos conserva el nivel de su rol"
        );
    }
}
