//! Integration tests for mesh peer and workspace authorization via the production router.

xavier::isolate_test_process!();

mod router_harness;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use router_harness::{jwt, status, test_app};
use tower::ServiceExt;
use xavier::mesh::node::NodeIdentity;
use xavier::security::auth::UserRole;

/// POST `uri` with `body` serialized as JSON and an optional bearer token.
///
/// `router_harness::status` always sends an empty body; the mesh workspace
/// query route needs a real payload, so this reuses the same one-shot request
/// shape with a body.
async fn post_json(
    app: &Router,
    uri: &str,
    body: serde_json::Value,
    bearer: Option<&str>,
) -> StatusCode {
    let mut req = Request::builder()
        .uri(uri)
        .method(Method::POST)
        .header("Content-Type", "application/json");
    if let Some(token) = bearer {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    app.clone()
        .oneshot(
            req.body(Body::from(
                serde_json::to_vec(&body).expect("serialize body"),
            ))
            .expect("request"),
        )
        .await
        .expect("response")
        .status()
}

/// A SWAL capability token exactly as `share_workspace_handler` mints it
/// (`src/cli/handlers/mesh.rs`): base64 of `{"payload": ..., "signature": ...}`,
/// where the signature is the issuing node's Ed25519 over `payload`.
fn workspace_token(identity: &NodeIdentity, workspace_id: &str) -> String {
    let payload = serde_json::json!({
        "token_id": "router-harness-token",
        "node_id": identity.node_id.as_str(),
        "endpoint": "http://localhost",
        "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
        "workspace_id": workspace_id,
        "expires_at": chrono::Utc::now().timestamp() + 3600,
    });
    let payload_str = serde_json::to_string(&payload).expect("payload json");
    let signature = identity.sign(payload_str.as_bytes());
    let token = serde_json::json!({
        "payload": payload_str,
        "signature": xavier::crypto::hex_encode(&signature),
    });
    xavier::crypto::base64_encode(serde_json::to_vec(&token).expect("token json"))
}

/// Same token, with the signature replaced by garbage: a forgery attempt.
fn forged_workspace_token(token: &str) -> String {
    let decoded = xavier::crypto::base64_decode(token).expect("token base64");
    let mut token_data: serde_json::Value = serde_json::from_slice(&decoded).expect("token json");
    token_data["signature"] = serde_json::json!(xavier::crypto::hex_encode([0xAB_u8; 64]));
    xavier::crypto::base64_encode(serde_json::to_vec(&token_data).expect("token json"))
}

/// Every mutating mesh route behind the bearer-token gate answers 401 with no
/// credential at all. The router is the production one built by
/// `router_harness::test_app` (`xavier::cli::server::build_app`), not a local
/// copy of the route table.
///
/// `/v1/mesh/workspaces/query` is NOT in this list on purpose: it is a public
/// route in the production router (see `auth_middleware`'s public-path list in
/// `src/cli/http_setup.rs`) and is covered by
/// [`mesh_workspace_query_requires_a_signed_capability_token`] instead.
#[tokio::test]
async fn mutating_mesh_routes_without_token_return_401() {
    let t = test_app().await;

    let routes = vec![
        (Method::POST, "/v1/mesh/peers"),
        (Method::POST, "/v1/mesh/peers/pair"),
        (Method::POST, "/v1/mesh/peers/decode"),
        (Method::POST, "/v1/mesh/peers/generate-code"),
        (Method::PUT, "/v1/mesh/peers/some_node/acl"),
        (Method::DELETE, "/v1/mesh/peers/some_node"),
        (Method::POST, "/v1/mesh/workspaces/share"),
        (Method::POST, "/v1/mesh/workspaces/join"),
        (Method::POST, "/v1/mesh/consent/revoke"),
        (Method::POST, "/v1/mesh/bridges"),
        (Method::DELETE, "/v1/mesh/bridges/some_bridge"),
    ];

    for (method, path) in routes {
        assert_eq!(
            status(&t.app, method.clone(), path, None).await,
            StatusCode::UNAUTHORIZED,
            "Expected 401 for {} {}",
            method,
            path
        );
    }
}

/// The routes guarded by `require_permission(|r| r.can_edit_config())` answer
/// 403 — not 401 — for an authenticated user without that permission, which is
/// what proves the RBAC layer (and not just the token gate) is in the path.
#[tokio::test]
async fn mutating_mesh_peer_config_with_user_token_returns_403() {
    let t = test_app().await;
    // A regular User token (lacks `can_edit_config`)
    let user_token = jwt(UserRole::User);

    let routes = vec![
        (Method::POST, "/v1/mesh/peers"),
        (Method::PUT, "/v1/mesh/peers/some_node/acl"),
        (Method::DELETE, "/v1/mesh/peers/some_node"),
    ];

    for (method, path) in routes {
        assert_eq!(
            status(&t.app, method.clone(), path, Some(&user_token)).await,
            StatusCode::FORBIDDEN,
            "Expected 403 for {} {}",
            method,
            path
        );
    }
}

/// `/v1/mesh/workspaces/query` sits outside the bearer-token gate on purpose:
/// it is the federated-read surface a remote peer calls during sync, and peers
/// never hold this node's root/API token. `auth_middleware`
/// (`src/cli/http_setup.rs`) therefore lets it through without credentials, and
/// authorization is the Ed25519-signed SWAL capability token in the body.
///
/// So the matrix asserts THAT contract instead of a 401 the production router
/// cannot return: no token is rejected before any memory is read, a tampered
/// signature is a 401, and a correctly signed token clears the gate.
#[tokio::test]
async fn mesh_workspace_query_requires_a_signed_capability_token() {
    let t = test_app().await;
    const WORKSPACE_QUERY: &str = "/v1/mesh/workspaces/query";

    // No capability token at all: the handler cannot even decode one.
    assert_eq!(
        status(&t.app, Method::POST, WORKSPACE_QUERY, None).await,
        StatusCode::BAD_REQUEST,
        "A query without a capability token must not reach any memory",
    );

    let identity = NodeIdentity::generate();
    let valid = workspace_token(&identity, "test-ws");

    // A tampered signature must never pass as a valid grant.
    assert_eq!(
        post_json(
            &t.app,
            WORKSPACE_QUERY,
            serde_json::json!({ "token": forged_workspace_token(&valid), "query": "hello" }),
            None,
        )
        .await,
        StatusCode::UNAUTHORIZED,
        "A forged capability token must be rejected with 401",
    );

    // A correctly signed token clears the signature gate and is served.
    assert_eq!(
        post_json(
            &t.app,
            WORKSPACE_QUERY,
            serde_json::json!({ "token": valid, "query": "hello" }),
            None,
        )
        .await,
        StatusCode::OK,
        "A signed capability token must be accepted without a bearer token",
    );
}
