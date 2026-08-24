use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;
use xavier::server::maloca::data_node::{
    ConsentUpdateRequest, DataNodeConsentResponse, DataNodeManager,
};

#[derive(Debug, Serialize, Deserialize)]
struct RegistryResponse {
    apps: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlignmentResponse {
    goals_count: usize,
    compliant: bool,
    goals: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BacklogResponse {
    features: Vec<Value>,
    total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct InferRequest {
    prompt: String,
    model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct InferResponse {
    response: String,
    model: String,
    provider: String,
}

struct TestContext {
    base_url: String,
    _temp_dir: TempDir,
}

async fn handle_registry() -> Json<RegistryResponse> {
    Json(RegistryResponse {
        apps: vec![
            json!({"id": "maloca-ui", "name": "Maloca UI", "status": "active"}),
            json!({"id": "maloca-pwa", "name": "Maloca PWA", "status": "planned"}),
            json!({"id": "xavier-node", "name": "Xavier Cognitive Runtime", "status": "active"}),
        ],
    })
}

async fn handle_alignment() -> Json<AlignmentResponse> {
    let goals: Vec<Value> = (1..=12)
        .map(|i| {
            json!({
                "goal_id": format!("GOAL-{}", i),
                "title": format!("SWAL Goal {}", i),
                "compliant": true,
                "score": 100
            })
        })
        .collect();

    Json(AlignmentResponse {
        goals_count: goals.len(),
        compliant: true,
        goals,
    })
}

async fn handle_backlog_unified() -> Json<BacklogResponse> {
    let features = vec![
        json!({"id": "feat-mesh-service-network", "title": "Service Network", "status": "done"}),
        json!({"id": "feat-content-redaction", "title": "Content Redaction", "status": "done"}),
        json!({"id": "feat-human-curation", "title": "Human Curation", "status": "done"}),
        json!({"id": "feat-maloca-http-e2e", "title": "Maloca Unified HTTP E2E", "status": "in_progress"}),
    ];

    Json(BacklogResponse {
        total: features.len(),
        features,
    })
}

async fn handle_models_infer(Json(payload): Json<InferRequest>) -> Json<InferResponse> {
    let model = payload.model.unwrap_or_else(|| "local-mock-v1".to_string());
    Json(InferResponse {
        response: format!("Mock inference output for prompt: '{}'", payload.prompt),
        model,
        provider: "local-mock-provider".to_string(),
    })
}

async fn setup_test_server() -> TestContext {
    let temp_dir = TempDir::new().unwrap();
    let consent_path = temp_dir.path().join("consent.json");

    let manager = DataNodeManager::default().with_file_path(consent_path);

    let maloca_v1_router = Router::new()
        .route("/registry", get(handle_registry))
        .route("/alignment", get(handle_alignment))
        .route("/backlog/unified", get(handle_backlog_unified))
        .route("/models/infer", post(handle_models_infer))
        .route(
            "/node/consent",
            get(xavier::server::maloca::data_node::get_consent_handler)
                .post(xavier::server::maloca::data_node::update_consent_handler),
        )
        .with_state(manager);

    let app = Router::new().nest("/v1/maloca", maloca_v1_router);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestContext {
        base_url: format!("http://{}", addr),
        _temp_dir: temp_dir,
    }
}

#[tokio::test]
async fn test_maloca_http_e2e_registry() {
    let ctx = setup_test_server().await;
    let client = Client::new();

    let res = client
        .get(format!("{}/v1/maloca/registry", ctx.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);

    let body: RegistryResponse = res.json().await.unwrap();
    assert!(!body.apps.is_empty());
    assert_eq!(body.apps.len(), 3);
    assert_eq!(body.apps[0]["id"], "maloca-ui");
}

#[tokio::test]
async fn test_maloca_http_e2e_alignment() {
    let ctx = setup_test_server().await;
    let client = Client::new();

    let res = client
        .get(format!("{}/v1/maloca/alignment", ctx.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);

    let body: AlignmentResponse = res.json().await.unwrap();
    assert_eq!(body.goals_count, 12);
    assert!(body.compliant);
    assert_eq!(body.goals.len(), 12);
}

#[tokio::test]
async fn test_maloca_http_e2e_backlog_unified() {
    let ctx = setup_test_server().await;
    let client = Client::new();

    let res = client
        .get(format!("{}/v1/maloca/backlog/unified", ctx.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);

    let body: BacklogResponse = res.json().await.unwrap();
    assert!(body.total >= 4);
    assert_eq!(body.features.len(), body.total);
}

#[tokio::test]
async fn test_maloca_http_e2e_models_infer() {
    let ctx = setup_test_server().await;
    let client = Client::new();

    let req = InferRequest {
        prompt: "Synthesize SWAL node consensus status".to_string(),
        model: Some("llama3-swal-fine-tuned".to_string()),
    };

    let res = client
        .post(format!("{}/v1/maloca/models/infer", ctx.base_url))
        .json(&req)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);

    let body: InferResponse = res.json().await.unwrap();
    assert!(body.response.contains("Synthesize SWAL node consensus status"));
    assert_eq!(body.model, "llama3-swal-fine-tuned");
    assert_eq!(body.provider, "local-mock-provider");
}

#[tokio::test]
async fn test_maloca_http_e2e_node_consent() {
    let ctx = setup_test_server().await;
    let client = Client::new();

    // 1. GET /v1/maloca/node/consent
    let get_res = client
        .get(format!("{}/v1/maloca/node/consent", ctx.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(get_res.status(), 200);
    let get_body: DataNodeConsentResponse = get_res.json().await.unwrap();
    assert!(!get_body.opt_in);
    assert_eq!(get_body.storage_quota_mb, 1024);
    assert_eq!(get_body.status, "opted_out");

    // 2. POST /v1/maloca/node/consent
    let update_req = ConsentUpdateRequest {
        opt_in: Some(true),
        storage_quota_mb: Some(4096),
    };

    let post_res = client
        .post(format!("{}/v1/maloca/node/consent", ctx.base_url))
        .json(&update_req)
        .send()
        .await
        .unwrap();

    assert_eq!(post_res.status(), 200);
    let post_body: DataNodeConsentResponse = post_res.json().await.unwrap();
    assert!(post_body.opt_in);
    assert_eq!(post_body.storage_quota_mb, 4096);
    assert_eq!(post_body.storage_quota_bytes, 4096 * 1024 * 1024);
    assert_eq!(post_body.status, "opted_in");

    // 3. Verify GET reflects updated consent state
    let get_res_2 = client
        .get(format!("{}/v1/maloca/node/consent", ctx.base_url))
        .send()
        .await
        .unwrap();

    assert_eq!(get_res_2.status(), 200);
    let get_body_2: DataNodeConsentResponse = get_res_2.json().await.unwrap();
    assert!(get_body_2.opt_in);
    assert_eq!(get_body_2.storage_quota_mb, 4096);
}
