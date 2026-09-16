//! E2E de la capa de clasificación (F4, feature `feat-classified-clearance`).
//!
//! Ejercita el **middleware real** contra dos identidades autenticadas, sin
//! servidor vivo: lo que se prueba es la cadena completa de decisión —
//! derivación del nivel desde las claims, exigencia por configuración de ruta y
//! auditoría de la denegación.
//!
//! Cubre los criterios que los unit tests no pueden cubrir: que el nivel que ve
//! el handler sea el derivado de la identidad, y que una ruta con exigencia
//! configurada rechace a quien no llega **y deje rastro**.

use axum::{
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    routing::get,
    Json, Router,
};
use tower::ServiceExt;
use xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware;
use xavier::security::auth::{Claims, UserRole};
use xavier::security::clearance::ClearanceLevel;

fn claims(sub: &str, email: &str, role: UserRole) -> Claims {
    Claims::new(
        sub.to_string(),
        email.to_string(),
        role,
        chrono::Duration::hours(1),
    )
}

fn request_with(path: &str, claims: Option<Claims>) -> Request<Body> {
    let mut request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("request válido");
    if let Some(claims) = claims {
        request.extensions_mut().insert(claims);
    }
    request
}

/// Router mínimo con el middleware real: un endpoint que devuelve el nivel que
/// quedó publicado en las extensiones y otro bajo un prefijo con exigencia.
fn app() -> Router {
    Router::new()
        .route(
            "/level",
            get(|Extension(level): Extension<ClearanceLevel>| async move {
                Json(serde_json::json!({ "level": format!("{level:?}") }))
            }),
        )
        .route(
            "/segments/seg-research/plan",
            get(|| async { "plan interno" }),
        )
        .layer(axum::middleware::from_fn(clearance_session_middleware))
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body legible");
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn two_identities_get_their_own_ceiling_and_route_policy_leaves_a_trail() {
    // Aislamiento: la auditoría va a un archivo temporal y la política de ruta
    // se configura por entorno (no toca el workspace).
    let audit_file = tempfile::NamedTempFile::new().expect("tempfile");
    std::env::set_var(
        xavier::security::clearance_audit::CLEARANCE_AUDIT_ENV,
        audit_file.path().to_string_lossy().to_string(),
    );
    std::env::set_var(
        xavier::security::route_policy::ROUTE_POLICY_ENV,
        r#"{"routes":[{"prefix":"/segments","required":"SECRET"}]}"#,
    );

    let app = app();

    // 1) Readonly ⇒ nivel INTERNAL (derivado del rol, no de un header).
    let readonly = claims("nodo-readonly", "ro@swal.dev", UserRole::Readonly);
    let response = app
        .clone()
        .oneshot(request_with("/level", Some(readonly.clone())))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);
    let payload = body_json(response).await;
    assert_eq!(payload["level"], "Internal");

    // 2) Admin ⇒ nivel TOP_SECRET.
    let admin = claims("nodo-admin", "admin@swal.dev", UserRole::Admin);
    let response = app
        .clone()
        .oneshot(request_with("/level", Some(admin.clone())))
        .await
        .expect("respuesta");
    let payload = body_json(response).await;
    assert_eq!(
        payload["level"], "TopSecret",
        "el mismo endpoint devuelve un techo distinto según la identidad"
    );

    // 3) La ruta que exige SECRET rechaza al Readonly (INTERNAL).
    let response = app
        .clone()
        .oneshot(request_with("/segments/seg-research/plan", Some(readonly)))
        .await
        .expect("respuesta");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "la ruta exige SECRET por configuración, no por header del cliente"
    );

    // 4) …y el Admin sí pasa.
    let response = app
        .clone()
        .oneshot(request_with("/segments/seg-research/plan", Some(admin)))
        .await
        .expect("respuesta");
    assert_eq!(response.status(), StatusCode::OK);

    // 5) La denegación quedó auditada, con la identidad y la ruta.
    let audit = std::fs::read_to_string(audit_file.path()).expect("auditoría escrita");
    assert!(
        audit.contains("route_denied"),
        "la denegación debe quedar auditada: {audit}"
    );
    assert!(audit.contains("nodo-readonly"), "con sujeto: {audit}");
    assert!(
        audit.contains("/segments/seg-research/plan"),
        "con ruta: {audit}"
    );
    assert!(
        !audit.contains("nodo-admin"),
        "una lectura permitida no debe auditarse como denegada: {audit}"
    );

    std::env::remove_var(xavier::security::route_policy::ROUTE_POLICY_ENV);
    std::env::remove_var(xavier::security::clearance_audit::CLEARANCE_AUDIT_ENV);
}

#[tokio::test]
async fn anonymous_never_reaches_above_unclassified() {
    let audit_file = tempfile::NamedTempFile::new().expect("tempfile");
    std::env::set_var(
        xavier::security::clearance_audit::CLEARANCE_AUDIT_ENV,
        audit_file.path().to_string_lossy().to_string(),
    );

    // Sin claims: el techo es UNCLASSIFIED aunque el header pida TOP_SECRET.
    let request = Request::builder()
        .uri("/level")
        .header("x-clearance", "TOP_SECRET")
        .body(Body::empty())
        .expect("request válido");

    let response = app().oneshot(request).await.expect("respuesta");
    let payload = body_json(response).await;
    assert_eq!(
        payload["level"], "Unclassified",
        "un anónimo no eleva su nivel con un header"
    );

    std::env::remove_var(xavier::security::clearance_audit::CLEARANCE_AUDIT_ENV);
}
