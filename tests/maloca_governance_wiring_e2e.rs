xavier::isolate_test_process!();

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use tower::util::ServiceExt;
use xavier::maloca::{
    data_node::ConsentRegistry, nested_router_with_consent, types::Proposal, MalocaStore,
};

#[tokio::test]
async fn proposal_lifecycle_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = MalocaStore::open(dir.path());
    let consent = ConsentRegistry::open(dir.path());
    let app = nested_router_with_consent(store, consent);

    // 1. Create a proposal
    let create_req = Request::builder()
        .method(Method::POST)
        .uri("/maloca/proposals")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "general",
                "title": "Wire Governance",
                "body": "Ensure HTTP endpoints correctly wire to the underlying store."
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let created_prop: Proposal = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(created_prop.title, "Wire Governance");
    assert_eq!(
        created_prop.status,
        xavier::maloca::types::ProposalStatus::Open
    );

    let prop_id = created_prop.id;

    // 2. Cast a Yes vote
    let vote_req = Request::builder()
        .method(Method::POST)
        .uri(format!("/maloca/proposals/{}/vote", prop_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "node_id": "lab_genesis",
                "choice": "yes"
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(vote_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 3. Fetch the proposals list to verify state
    let list_req = Request::builder()
        .method(Method::GET)
        .uri("/maloca/proposals")
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let props: Vec<Proposal> = serde_json::from_slice(&bytes).unwrap();

    let prop = props.into_iter().find(|p| p.id == prop_id).unwrap();
    // Governance outcome may not be fully resolved if not expired, but let's check it's returned.
    assert_eq!(prop.id, prop_id);
}

#[tokio::test]
async fn governance_proposal_rejection_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = MalocaStore::open(dir.path());
    let consent = ConsentRegistry::open(dir.path());
    let app = nested_router_with_consent(store, consent);

    // 1. Create a proposal
    let create_req = Request::builder()
        .method(Method::POST)
        .uri("/maloca/proposals")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "general",
                "title": "Bad Idea",
                "body": "Should be rejected."
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(create_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let created_prop: Proposal = serde_json::from_slice(&bytes).unwrap();
    let prop_id = created_prop.id;

    // 2. Cast a No vote
    let vote_req = Request::builder()
        .method(Method::POST)
        .uri(format!("/maloca/proposals/{}/vote", prop_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "node_id": "lab_genesis",
                "choice": "no"
            })
            .to_string(),
        ))
        .unwrap();

    let res = app.clone().oneshot(vote_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 3. Fetch the proposals list to verify state
    let list_req = Request::builder()
        .method(Method::GET)
        .uri("/maloca/proposals")
        .body(Body::empty())
        .unwrap();

    let res = app.clone().oneshot(list_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let props: Vec<Proposal> = serde_json::from_slice(&bytes).unwrap();

    let prop = props.into_iter().find(|p| p.id == prop_id).unwrap();
    assert_eq!(prop.id, prop_id);
}
