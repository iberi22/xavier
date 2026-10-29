xavier::isolate_test_process!();

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use chrono::{Duration, Utc};
use http_body_util::BodyExt;
use serde_json::json;
use std::fs;
use tower::util::ServiceExt;
use xavier::maloca::{
    data_node::ConsentRegistry,
    nested_router_with_consent,
    types::{Proposal, ProposalStatus},
    MalocaStore,
};

fn read_store_json(dir: &tempfile::TempDir) -> serde_json::Value {
    let path = dir.path().join("maloca").join("store.json");
    let content = fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn write_store_json(dir: &tempfile::TempDir, json: serde_json::Value) {
    let path = dir.path().join("maloca").join("store.json");
    fs::write(path, serde_json::to_string(&json).unwrap()).unwrap();
}

#[tokio::test]
async fn proposal_lifecycle_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = MalocaStore::open(dir.path());
    let consent = ConsentRegistry::open(dir.path());
    let app: axum::Router<()> = nested_router_with_consent(store.clone(), consent.clone());

    // 1. Create a proposal. This implicitly flushes `store.json` to disk.
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
    assert_eq!(created_prop.status, ProposalStatus::Open);

    let prop_id = created_prop.id.clone();

    // Now we can safely modify store.json
    let mut state = read_store_json(&dir);
    // Make lab_genesis eligible by setting karma to 600
    state["nodes"] = json!([{
        "node_id": "lab_genesis",
        "role": "peer",
        "karma": 600,
        "active": true,
        "note": ""
    }]);
    write_store_json(&dir, state);

    // Reopen store to load karma
    let store = MalocaStore::open(dir.path());
    let app: axum::Router<()> = nested_router_with_consent(store.clone(), consent.clone());

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

    // 3. Fast-forward time to expire it
    let mut state = read_store_json(&dir);
    for p in state["proposals"].as_array_mut().unwrap() {
        if p["id"] == prop_id {
            p["expires_at"] = json!((Utc::now() - Duration::days(1)).to_rfc3339());
        }
    }
    write_store_json(&dir, state);

    // Reopen store to load modified expiry
    let store = MalocaStore::open(dir.path());
    let app: axum::Router<()> = nested_router_with_consent(store.clone(), consent.clone());

    // 4. Fetch the proposals list to trigger close_due_proposals
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
    assert_eq!(prop.status, ProposalStatus::Closed);
    assert_eq!(prop.outcome.as_deref(), Some("approved"));

    // 5. Verify the decision persisted
    let reopened_store = MalocaStore::open(dir.path());
    let persisted_prop = reopened_store
        .list_proposals()
        .into_iter()
        .find(|p| p.id == prop_id)
        .unwrap();
    assert_eq!(persisted_prop.status, ProposalStatus::Closed);
    assert_eq!(persisted_prop.outcome.as_deref(), Some("approved"));

    let decisions = reopened_store.list_decisions();
    assert!(decisions
        .iter()
        .any(|d| d.kind == xavier::maloca::types::DecisionKind::Closed
            && d.proposal_id.as_deref() == Some(prop_id.as_str())));
}

#[tokio::test]
async fn governance_proposal_rejection_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = MalocaStore::open(dir.path());
    let consent = ConsentRegistry::open(dir.path());
    let app: axum::Router<()> = nested_router_with_consent(store.clone(), consent.clone());

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
    let prop_id = created_prop.id.clone();

    let mut state = read_store_json(&dir);
    state["nodes"] = json!([{
        "node_id": "lab_genesis",
        "role": "peer",
        "karma": 600,
        "active": true,
        "note": ""
    }]);
    write_store_json(&dir, state);

    let store = MalocaStore::open(dir.path());
    let app: axum::Router<()> = nested_router_with_consent(store.clone(), consent.clone());

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

    // 3. Fast-forward time to expire it
    let mut state = read_store_json(&dir);
    for p in state["proposals"].as_array_mut().unwrap() {
        if p["id"] == prop_id {
            p["expires_at"] = json!((Utc::now() - Duration::days(1)).to_rfc3339());
        }
    }
    write_store_json(&dir, state);

    let store = MalocaStore::open(dir.path());
    let app: axum::Router<()> = nested_router_with_consent(store.clone(), consent.clone());

    // 4. Fetch the proposals list to trigger close_due_proposals
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
    assert_eq!(prop.status, ProposalStatus::Closed);
    assert_eq!(prop.outcome.as_deref(), Some("rejected"));

    // 5. Verify the decision persisted
    let reopened_store = MalocaStore::open(dir.path());
    let persisted_prop = reopened_store
        .list_proposals()
        .into_iter()
        .find(|p| p.id == prop_id)
        .unwrap();
    assert_eq!(persisted_prop.status, ProposalStatus::Closed);
    assert_eq!(persisted_prop.outcome.as_deref(), Some("rejected"));

    let decisions = reopened_store.list_decisions();
    assert!(decisions
        .iter()
        .any(|d| d.kind == xavier::maloca::types::DecisionKind::Closed
            && d.proposal_id.as_deref() == Some(prop_id.as_str())));
}
