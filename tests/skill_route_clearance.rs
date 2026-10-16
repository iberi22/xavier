use axum::{body::Body, http::Request, Extension, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;
use xavier::security::{
    auth::{Claims, UserRole},
    clearance::ClearanceLevel,
};
use xavier::workspace::{WorkspaceContext, WorkspaceState};

xavier::isolate_test_process!();

const TASK: &str = "Design distributed mesh consensus protocols and architecture";
const PRIVATE: &str = "clearance-mesh-architect";
const MARKER: &str = "CONFIDENTIAL-INSTRUCTION-7d9a";

struct Fixture {
    workspace: WorkspaceContext,
    _root: tempfile::TempDir,
    previous_store: Option<std::ffi::OsString>,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("store");
        for (name, description, clearance, body) in [
            (PRIVATE, TASK, "confidential", MARKER),
            (
                "public-baking",
                "Bake bread and pastries",
                "unclassified",
                "Public instructions",
            ),
        ] {
            let dir = store.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("SKILL.md"), format!(
                "---\nname: {name}\ndescription: \"{description}\"\nclearance: {clearance}\n---\n{body}\n"
            )).unwrap();
        }
        let previous_store = std::env::var_os("XAVIER_SKILL_STORE");
        std::env::set_var("XAVIER_SKILL_STORE", &store);
        let memory = Arc::new(xavier::memory::qmd_memory::QmdMemory::new(Arc::new(
            tokio::sync::RwLock::new(Vec::new()),
        )));
        let state = WorkspaceState::new_minimal(
            "clearance-test".into(),
            root.path().to_path_buf(),
            memory,
            Arc::new(xavier::memory::store::InMemoryMemoryStore::default()),
        )
        .await;
        Self {
            workspace: WorkspaceContext {
                workspace_id: root.path().to_string_lossy().into_owned(),
                workspace: Arc::new(state),
            },
            _root: root,
            previous_store,
        }
    }

    fn router(&self, with_clearance: bool) -> Router {
        let routes = xavier::api::skills::skill_api_routes()
            .merge(xavier::server::http::context::context_regenerate_routes())
            .layer(Extension(self.workspace.clone()));
        if with_clearance {
            routes.layer(axum::middleware::from_fn(
                xavier::adapters::inbound::http::middleware::clearance::clearance_session_middleware,
            ))
        } else {
            routes
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match self.previous_store.take() {
            Some(value) => std::env::set_var("XAVIER_SKILL_STORE", value),
            None => std::env::remove_var("XAVIER_SKILL_STORE"),
        }
    }
}

async fn request(
    app: &Router,
    path: &str,
    payload: Option<Value>,
    role: Option<UserRole>,
) -> Value {
    let mut req = Request::builder()
        .uri(path)
        .method(if payload.is_some() { "POST" } else { "GET" })
        .header("content-type", "application/json")
        .body(Body::from(
            payload.map(|p| p.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    if let Some(role) = role {
        req.extensions_mut().insert(Claims::new(
            "fixture-caller".into(),
            "fixture@example.invalid".into(),
            role,
            chrono::Duration::hours(1),
        ));
    }
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
#[serial_test::serial]
async fn skill_lists_enforce_caller_clearance() {
    xavier::isolate_test_process!();
    let f = Fixture::new().await;
    for path in ["/api/skill/list", "/skills"] {
        for role in [
            None,
            Some(UserRole::Readonly),
            Some(UserRole::User),
            Some(UserRole::Admin),
        ] {
            let body = request(&f.router(true), path, None, role).await;
            let allowed = matches!(role, Some(UserRole::User | UserRole::Admin));
            assert_eq!(
                body.to_string().contains(PRIVATE),
                allowed,
                "{path}: {body}"
            );
            assert_eq!(
                body.to_string().contains(TASK),
                allowed,
                "description: {body}"
            );
            assert_eq!(body["count"], if allowed { 2 } else { 1 });
            assert!(body.to_string().contains("public-baking"));
        }
        let fallback = request(&f.router(false), path, None, None).await;
        assert!(!fallback.to_string().contains(PRIVATE));
    }
}

#[tokio::test]
#[serial_test::serial]
async fn skill_rest_dispatch_enforces_caller_clearance() {
    xavier::isolate_test_process!();
    let f = Fixture::new().await;
    for role in [
        None,
        Some(UserRole::Readonly),
        Some(UserRole::User),
        Some(UserRole::Admin),
    ] {
        let body = request(
            &f.router(true),
            "/api/skill/dispatch",
            Some(json!({"task": TASK})),
            role,
        )
        .await;
        let allowed = matches!(role, Some(UserRole::User | UserRole::Admin));
        assert_eq!(body["data"]["skill_name"] == PRIVATE, allowed, "{body}");
        assert_eq!(body.to_string().contains(PRIVATE), allowed, "{body}");
    }
    let body = request(
        &f.router(false),
        "/api/skill/dispatch",
        Some(json!({"task": TASK})),
        None,
    )
    .await;
    assert!(!body.to_string().contains(PRIVATE));
}

#[tokio::test]
#[serial_test::serial]
async fn skill_context_fusion_enforces_caller_clearance() {
    xavier::isolate_test_process!();
    let f = Fixture::new().await;
    let thread = f
        .workspace
        .workspace
        .conversations_db
        .create_thread(Some("clearance"), None, Some("test"))
        .await
        .unwrap();
    f.workspace
        .workspace
        .conversations_db
        .store_message(&thread.id, "user", TASK, None, None, None, None, Some(20))
        .await
        .unwrap();
    for role in [
        None,
        Some(UserRole::Readonly),
        Some(UserRole::User),
        Some(UserRole::Admin),
    ] {
        let body = request(
            &f.router(true),
            "/v1/context/regenerate",
            Some(json!({"session_id": thread.id, "depth": "deep"})),
            role,
        )
        .await;
        let allowed = matches!(role, Some(UserRole::User | UserRole::Admin));
        let context = body["context"].as_str().unwrap();
        assert_eq!(context.contains(PRIVATE), allowed, "{body}");
        assert_eq!(context.contains(PRIVATE), allowed, "{body}");
    }
    let body = request(
        &f.router(false),
        "/v1/context/regenerate",
        Some(json!({"session_id": thread.id, "depth": "deep"})),
        None,
    )
    .await;
    assert!(!body.to_string().contains(PRIVATE));
}

#[test]
fn skill_transport_default_matches_memory_reads() {
    xavier::isolate_test_process!();
    assert_eq!(
        xavier::security::clearance::default_clearance(),
        ClearanceLevel::Internal
    );
    assert_eq!(
        xavier::server::mcp::tools_memory::caller_clearance(),
        ClearanceLevel::Unclassified
    );
}

#[tokio::test]
#[serial_test::serial]
async fn skill_dispatch_rejects_weak_matches_and_counts_invalid_entries() {
    use tracing::instrument::WithSubscriber;
    use xavier::context::{
        skill_dispatcher::MIN_DISPATCH_CONFIDENCE, skill_registry::SkillRegistry,
    };

    xavier::isolate_test_process!();
    let f = Fixture::new().await;
    let store = f._root.path().join("store");
    for (folder, name, description) in [
        (
            "trace-audit",
            "trace-audit",
            "Audit the failed traces of a run",
        ),
        (
            "not-a-skill",
            "my skill",
            "Audit the failed traces of a run",
        ),
    ] {
        let dir = store.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: \"{name}\"\ndescription: \"{description}\"\n---\nContent"),
        )
        .unwrap();
    }

    // Rejection counts are exposed in registry tracing, not in the HTTP response.
    let log = tempfile::tempfile().unwrap();
    let writer = log.try_clone().unwrap();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.try_clone().unwrap())
        .finish();
    let body = async {
        let mut registry = SkillRegistry::new(vec![store]);
        assert_eq!(registry.reindex().await.unwrap(), 3);
        assert_eq!(registry.len(), 3);
        assert!(registry.get("my skill").is_none());
        let candidates = registry.search("failed", 10);
        assert_eq!(candidates.len(), 1);
        let score = candidates[0].0;
        assert!(score > 0.0 && score < MIN_DISPATCH_CONFIDENCE, "{score}");

        request(
            &f.router(false),
            "/api/skill/dispatch",
            Some(json!({"task": "failed"})),
            None,
        )
        .await
    }
    .with_subscriber(subscriber)
    .await;
    assert_eq!(body["ok"], true);
    let data = &body["data"];
    assert_eq!(data["skill_name"], "_none");
    assert!(data["confidence"].as_f64().unwrap() < f64::from(MIN_DISPATCH_CONFIDENCE));
    assert_eq!(data["context_pack"]["system_instructions"], "");
    assert_eq!(data["context_pack"]["total_tokens"], 0);

    use std::io::{Read, Seek, SeekFrom};
    let mut log = log;
    log.seek(SeekFrom::Start(0)).unwrap();
    let mut output = String::new();
    log.read_to_string(&mut output).unwrap();
    assert!(
        output.contains("3 skills indexed, 1 rejected, 3 total"),
        "{output}"
    );
}
