use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use xavier::adapters::inbound::http::handlers::ivn::{
    init_ivn_engine, IvnEngineStore, ValidatorCandidateDto,
};
use xavier::adapters::inbound::http::routes::create_router;
use xavier::data_commons::ivn::Vote;

#[tokio::test]
async fn test_ivn_api_full_lifecycle() {
    // Reset IVN engine store for test isolation
    init_ivn_engine(IvnEngineStore::default());

    let router = create_router();

    // 1. Create Identity Verification Request
    let candidates = vec![
        ValidatorCandidateDto {
            node_id: "val_node_1".into(),
            wallet: "xv1_wallet_val1".into(),
            karma: 500,
            seed: "seed_1".into(),
        },
        ValidatorCandidateDto {
            node_id: "val_node_2".into(),
            wallet: "xv1_wallet_val2".into(),
            karma: 600,
            seed: "seed_2".into(),
        },
        ValidatorCandidateDto {
            node_id: "val_node_3".into(),
            wallet: "xv1_wallet_val3".into(),
            karma: 700,
            seed: "seed_3".into(),
        },
        ValidatorCandidateDto {
            node_id: "val_node_4".into(),
            wallet: "xv1_wallet_val4".into(),
            karma: 800,
            seed: "seed_4".into(),
        },
        ValidatorCandidateDto {
            node_id: "val_node_5".into(),
            wallet: "xv1_wallet_val5".into(),
            karma: 900,
            seed: "seed_5".into(),
        },
        ValidatorCandidateDto {
            node_id: "val_node_6".into(),
            wallet: "xv1_wallet_val6".into(),
            karma: 1000,
            seed: "seed_6".into(),
        },
    ];

    let create_payload = json!({
        "applicant": "xv1_applicant_wallet_123",
        "proof_hashes": ["sha256:proof_hash_1", "sha256:proof_hash_2"],
        "signature": "ml_dsa_65_signature_hex",
        "candidate_pool": candidates,
        "seed": "applicant_seed_123"
    });

    let create_req = Request::builder()
        .uri("/v1/identity/request")
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&create_payload).unwrap()))
        .unwrap();

    let response = router.clone().oneshot(create_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let create_res: Value = serde_json::from_slice(&body_bytes).unwrap();

    assert_eq!(create_res["status"], "ok");
    let req_id = create_res["request"]["id"].as_str().unwrap().to_string();
    assert!(req_id.starts_with("ivn_req_"));
    assert_eq!(create_res["request"]["status"], "pending");

    let assigned_validators = create_res["request"]["assigned_validators"]
        .as_array()
        .unwrap();
    assert_eq!(assigned_validators.len(), 5);

    let assigned_node_ids: Vec<String> = assigned_validators
        .iter()
        .map(|v| v["node_id"].as_str().unwrap().to_string())
        .collect();

    // 2. GET request status by ID
    let get_req = Request::builder()
        .uri(format!("/v1/identity/request/{}", req_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(get_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let get_res: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(get_res["status"], "ok");
    assert_eq!(get_res["request"]["id"], req_id);

    // 3. Attempt Vote from Unauthorized Validator Node (403 FORBIDDEN)
    let unauthorized_vote = json!({
        "validator_node_id": "unauthorized_malicious_node",
        "vote": "Check"
    });

    let unauth_req = Request::builder()
        .uri(format!("/v1/identity/{}/vote", req_id))
        .method(Method::POST)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&unauthorized_vote).unwrap()))
        .unwrap();

    let response = router.clone().oneshot(unauth_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // 4. Authorized Validators Vote (4 Check votes, 1 Reject vote => 80% Check >= 0.8 quorum threshold => Passed)
    for (idx, node_id) in assigned_node_ids.iter().enumerate() {
        let vote_choice = if idx < 4 { Vote::Check } else { Vote::Reject };
        let vote_payload = json!({
            "validator_node_id": node_id,
            "vote": vote_choice
        });

        let vote_req = Request::builder()
            .uri(format!("/v1/identity/{}/vote", req_id))
            .method(Method::POST)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&vote_payload).unwrap()))
            .unwrap();

        let response = router.clone().oneshot(vote_req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Check request status after all 5 votes
    let check_req = Request::builder()
        .uri(format!("/v1/identity/request/{}", req_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(check_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let updated_res: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(updated_res["request"]["status"], "passed");
    assert_eq!(updated_res["request"]["verdict"]["check_count"], 4);

    // 5. List Requests (Paginated)
    let list_req = Request::builder()
        .uri("/v1/identity/requests?page=1&limit=10")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(list_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let list_res: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(list_res["status"], "ok");
    assert_eq!(list_res["page"], 1);
    assert_eq!(list_res["limit"], 10);
    assert!(list_res["total"].as_u64().unwrap() >= 1);

    // 6. List Verified Nodes
    let verified_req = Request::builder()
        .uri("/v1/identity/verified")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();

    let response = router.clone().oneshot(verified_req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();
    let verified_res: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(verified_res["status"], "ok");
    assert_eq!(verified_res["count"], 1);
    assert_eq!(
        verified_res["verified_nodes"][0]["applicant"],
        "xv1_applicant_wallet_123"
    );
}
