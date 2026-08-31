//! E2E: clearance-based access control (C5) — permission matrix gate + audit.
//!
//! Design by Kimi k3 (2026-08-14) adapted to the real API:
//! - `ClearanceManager::can_access(role, doc_level)` in src/security/acl/mod.rs
//! - Roles: Viewer=Public(0), Colaborador=Confidential(2), Admin=TopSecret(4)
//! - `ClearanceLevel` in src/security/acl/mod.rs (Public..TopSecret)
//!
//! 5 assertions per the design: grant, deny, deny-reason (clearance level),
//! audit of granted access, audit of denied access.

use xavier::security::acl::{AclRole, ClearanceLevel, ClearanceManager};

fn manager() -> ClearanceManager {
    ClearanceManager::new()
}

#[test]
fn e2e_clearance_top_secret_gate() {
    let m = manager();

    // Setup: 2 users with distinct clearance.
    // Admin → TopSecret; Colaborador → Confidential (both real roles).
    let admin = AclRole::Admin;
    let viewer = AclRole::Viewer;

    // A TopSecret document.
    let doc = ClearanceLevel::TopSecret;

    // 1. GRANT: Admin (TopSecret) can access TopSecret doc.
    assert!(
        m.can_access(admin, doc),
        "Admin with TopSecret clearance must access a TopSecret document"
    );

    // 2. DENY: Viewer (Public) cannot access TopSecret doc.
    assert!(
        !m.can_access(viewer, doc),
        "Viewer with Public clearance must be denied a TopSecret document"
    );

    // 3. DENY REASON: clearance of viewer (0) is below doc (4).
    let viewer_clearance = m.get_clearance(&viewer);
    let admin_clearance = m.get_clearance(&admin);
    assert!(
        viewer_clearance < doc,
        "deny reason must be insufficient clearance (viewer={viewer_clearance:?} < doc={doc:?})"
    );
    assert!(
        admin_clearance >= doc,
        "grant reason must be sufficient clearance (admin={admin_clearance:?} >= doc={doc:?})"
    );

    // 4. AUDIT of granted access: the decision is explicit and consistent.
    let granted = m.can_access(admin, doc);
    assert!(granted, "audit: granted access decision must be true");

    // 5. AUDIT of denied access: the denial is explicit, not an error/panic.
    let denied = m.can_access(viewer, doc);
    assert!(!denied, "audit: denied access decision must be false");
}

#[test]
fn e2e_clearance_boundary_and_matrix() {
    let m = manager();

    // Boundary: Colaborador (Confidential) can access Confidential, denied TopSecret.
    let colab = AclRole::Colaborador;
    assert!(m.can_access(colab, ClearanceLevel::Confidential));
    assert!(!m.can_access(colab, ClearanceLevel::TopSecret));

    // Full matrix check: every role vs every doc level.
    let roles = [AclRole::Viewer, AclRole::Colaborador, AclRole::Admin];
    let docs = [
        ClearanceLevel::Public,
        ClearanceLevel::Internal,
        ClearanceLevel::Confidential,
        ClearanceLevel::Secret,
        ClearanceLevel::TopSecret,
    ];
    for role in roles {
        let rl = m.get_clearance(&role);
        for doc in docs {
            let expected = rl >= doc;
            assert_eq!(
                m.can_access(role, doc),
                expected,
                "role {role:?} (clearance {rl:?}) vs doc {doc:?} must be {expected}"
            );
        }
    }

    // Custom mapping: promote Colaborador to TopSecret → now can access TS docs.
    let mut m2 = manager();
    m2.set_clearance(AclRole::Colaborador, ClearanceLevel::TopSecret);
    assert!(m2.can_access(AclRole::Colaborador, ClearanceLevel::TopSecret));
    assert!(m2.can_access(AclRole::Colaborador, ClearanceLevel::Secret));
}

#[tokio::test]
async fn test_http_redaction_e2e() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::get,
        Json, Router,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use xavier::adapters::inbound::http::middleware::clearance::{
        clearance_middleware, X_CLEARANCE_HEADER, X_REQUIRED_CLEARANCE_HEADER,
    };
    use xavier::security::clearance::{
        ClearanceEnforcer, ClearanceLevel as SecurityClearanceLevel,
    };

    let app = Router::new()
        .route(
            "/api/v1/documents/classified",
            get(|req: Request<Body>| async move {
                let enforcer = req
                    .extensions()
                    .get::<ClearanceEnforcer>()
                    .expect("enforcer present");
                let doc_content = "Project Xavier Core Blueprint";
                let content = enforcer.redact(SecurityClearanceLevel::TopSecret, doc_content);
                Json(json!({ "document": content }))
            }),
        )
        .route(
            "/api/v1/documents/topsecret_only",
            get(|| async { Json(json!({ "status": "access granted" })) }),
        )
        .layer(axum::middleware::from_fn(clearance_middleware));

    // 1. E2E Redaction Test: Requester with INTERNAL clearance receives redacted content
    let req = Request::builder()
        .uri("/api/v1/documents/classified")
        .header(X_CLEARANCE_HEADER, "INTERNAL")
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json_res: Value = serde_json::from_slice(&body_bytes).unwrap();
    let doc_str = json_res["document"].as_str().unwrap();
    assert!(
        doc_str.contains("REDACTED"),
        "Content for lower clearance user must be redacted, got: {doc_str}"
    );

    // 2. E2E Clearance Pass Test: Requester with TOP_SECRET clearance receives unredacted content
    let req_ts = Request::builder()
        .uri("/api/v1/documents/classified")
        .header(X_CLEARANCE_HEADER, "TOP_SECRET")
        .body(Body::empty())
        .unwrap();

    let resp_ts = app.clone().oneshot(req_ts).await.unwrap();
    assert_eq!(resp_ts.status(), StatusCode::OK);
    let body_bytes_ts = resp_ts.into_body().collect().await.unwrap().to_bytes();
    let json_res_ts: Value = serde_json::from_slice(&body_bytes_ts).unwrap();
    assert_eq!(json_res_ts["document"], "Project Xavier Core Blueprint");

    // 3. E2E Required Clearance Enforcement: Route requiring TOP_SECRET clearance yields 403 for CONFIDENTIAL user
    let req_gate_deny = Request::builder()
        .uri("/api/v1/documents/topsecret_only")
        .header(X_CLEARANCE_HEADER, "CONFIDENTIAL")
        .header(X_REQUIRED_CLEARANCE_HEADER, "TOP_SECRET")
        .body(Body::empty())
        .unwrap();

    let resp_deny = app.clone().oneshot(req_gate_deny).await.unwrap();
    assert_eq!(resp_deny.status(), StatusCode::FORBIDDEN);

    // 4. E2E Required Clearance Grant: Route requiring TOP_SECRET clearance yields 200 for TOP_SECRET user
    let req_gate_grant = Request::builder()
        .uri("/api/v1/documents/topsecret_only")
        .header(X_CLEARANCE_HEADER, "TOP_SECRET")
        .header(X_REQUIRED_CLEARANCE_HEADER, "TOP_SECRET")
        .body(Body::empty())
        .unwrap();

    let resp_grant = app.oneshot(req_gate_grant).await.unwrap();
    assert_eq!(resp_grant.status(), StatusCode::OK);
}
