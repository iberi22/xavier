//! feat-skill-registry-unify: REST and MCP skill listings come from one registry.
//! Fixture-only: HOME and XAVIER_SKILL_STORE point at tempdirs under a local env lock.

use axum::response::IntoResponse;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use xavier::agents::RuntimeConfig;
use xavier::context::skill_registry::build_skill_registry;
use xavier::memory::file_indexer::{FileIndexer, FileIndexerConfig};
use xavier::memory::store::MemoryBackend;
use xavier::workspace::{
    EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceContext,
    WorkspaceRegistry, WorkspaceState,
};
use xavier::AppState;

xavier::isolate_test_process!();

const SKILLS: [(&str, &str); 3] = [
    (
        "alpha-unify",
        "First fixture skill for registry unification",
    ),
    (
        "beta-unify",
        "Second fixture skill for registry unification",
    ),
    (
        "gamma-unify",
        "Third fixture skill for registry unification",
    ),
];

/// Serializes the process-global env.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct EnvGuard {
    prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn set(home: &Path, store: &Path) -> Self {
        let keys = ["HOME", "XAVIER_SKILL_STORE"];
        let prev = keys.iter().map(|k| (*k, std::env::var_os(k))).collect();
        std::env::set_var("HOME", home);
        std::env::set_var("XAVIER_SKILL_STORE", store);
        Self { prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in self.prev.drain(..) {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn write_skill(root: &Path, name: &str, description: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("skill dir");
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: \"{description}\"\nvisibility: public\n---\n\n# {name}\n\nFixture body.\n"
        ),
    )
    .expect("SKILL.md");
}

async fn fixture(tmp: &TempDir) -> (AppState, WorkspaceContext) {
    let config = WorkspaceConfig {
        id: "skill-unify-ws".to_string(),
        token: "unused".to_string(),
        plan: PlanTier::Community,
        memory_backend: MemoryBackend::Vec,
        storage_limit_bytes: None,
        request_limit: None,
        request_unit_limit: None,
        embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
        managed_google_embeddings: false,
        sync_policy: SyncPolicy::LocalOnly,
        dedup: Default::default(),
        protocol: Default::default(),
    };
    let ws = WorkspaceState::new_for_space(
        config,
        RuntimeConfig::default(),
        tmp.path().join("ws"),
        tmp.path().join("store"),
    )
    .await
    .expect("workspace");
    let workspace = WorkspaceContext {
        // The workspace root doubles as the id: both transports scan it.
        workspace_id: tmp.path().join("workspace").to_string_lossy().into_owned(),
        workspace: Arc::new(ws),
    };
    let code_db = Arc::new(
        code_graph::db::CodeGraphDB::new(&tmp.path().join("code_graph.db")).expect("code db"),
    );
    let code_indexer = Arc::new(code_graph::indexer::Indexer::new(Arc::clone(&code_db)));
    let code_query = Arc::new(code_graph::query::QueryEngine::new(Arc::clone(&code_db)));
    let state = AppState {
        workspace_registry: Arc::new(WorkspaceRegistry::new()),
        indexer: FileIndexer::new(FileIndexerConfig::default(), Some(code_indexer.clone())),
        agent_indexer: xavier::memory::agent_indexer::AgentIndexer::new(FileIndexer::new(
            FileIndexerConfig::default(),
            Some(code_indexer.clone()),
        )),
        code_indexer,
        code_query,
        code_db,
        security_service: Arc::new(xavier::app::security_service::SecurityService::new()),
        code_graph_dump_path: None,
        telecom: None,
    };
    (state, workspace)
}

fn names(payload: &Value) -> BTreeSet<String> {
    payload["skills"]
        .as_array()
        .expect("skills array")
        .iter()
        .map(|s| s["name"].as_str().expect("skill name").to_string())
        .collect()
}

#[tokio::test]
async fn rest_and_mcp_skill_lists_match_build_skill_registry() {
    let _lock = ENV_LOCK.lock().await;
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let store = tmp.path().join("skill-store");
    std::fs::create_dir_all(&home).unwrap();
    for (name, description) in SKILLS {
        write_skill(&store, name, description);
    }
    let _env = EnvGuard::set(&home, &store);

    let (state, workspace) = fixture(&tmp).await;

    // Source of truth: the single registry constructor.
    let registry = build_skill_registry(Path::new(&workspace.workspace_id)).await;
    let expected: BTreeSet<String> = registry.list().into_iter().map(String::from).collect();
    for (name, _) in SKILLS {
        assert!(expected.contains(name), "registry misses fixture {name}");
    }

    // REST: the production router.
    use tower::util::ServiceExt;
    let app =
        xavier::api::skills::skill_api_routes::<()>().layer(axum::Extension(workspace.clone()));
    for uri in ["/api/skill/list", "/skills"] {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .expect("rest response")
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK, "{uri}");
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("rest body");
        let rest: Value = serde_json::from_slice(&body).expect("rest json");
        assert_eq!(rest["count"], expected.len(), "{uri}");
        assert_eq!(names(&rest), expected, "{uri}");
    }

    // MCP: xavier_skill_list.
    let out = xavier::server::mcp::tools_context::handle_context_tool(
        state,
        workspace,
        "xavier_skill_list",
        json!({}),
    )
    .await
    .expect("mcp skill list never throws");
    let text = out["content"][0]["text"].as_str().expect("mcp text");
    let mcp: Value = serde_json::from_str(text).expect("mcp json");
    assert_eq!(mcp["ok"], true);
    assert_eq!(mcp["count"], expected.len());
    assert_eq!(names(&mcp), expected);
}
