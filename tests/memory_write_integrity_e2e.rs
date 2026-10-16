//! Exercise the public HTTP handlers without sockets, an embedder, or a daemon.

use axum::{body::Body, http::Request, routing::post, Extension, Router};
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use xavier::{
    memory::{
        connection_provider::InMemoryProvider,
        qmd_memory::{MemoryDocument, QmdMemory},
        sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig},
        store::{InMemoryMemoryStore, MemoryStore},
    },
    server::v1_api::{v1_memories_add, v1_memories_get},
    workspace::{WorkspaceContext, WorkspaceState},
};

xavier::isolate_test_process!();

const WORKSPACE: &str = "write-integrity-http";
const PATH: &str = "activity/integrity/events";
const CONTENT: &str = "The audit report was signed after all corrections were recorded.";

struct Fixture {
    app: Router,
    store: Arc<dyn MemoryStore>,
    docs: Arc<tokio::sync::RwLock<Vec<MemoryDocument>>>,
    _root: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_vec_store(false).await
    }

    async fn with_vec_store(use_vec_store: bool) -> Self {
        // All selectors must be disabled: an inherited invalid selector is
        // checked before the disabled provider mode in EmbedderConfig.
        // Keep this test binary offline through shutdown, including detached
        // vector-backfill tasks spawned by the real add handler.
        for key in [
            "XAVIER_EMBEDDER",
            "XAVIER_EMBEDDING_PROVIDER_MODE",
            "XAVIER_EMBED_PROVIDER",
        ] {
            std::env::set_var(key, "disabled");
        }
        let root = tempfile::tempdir().unwrap();
        let store: Arc<dyn MemoryStore> = if use_vec_store {
            Arc::new(
                VecSqliteMemoryStore::new_with_provider(
                    VecSqliteStoreConfig {
                        path: root.path().join("memory.db"),
                        embedding_dimensions: 3,
                    },
                    Arc::new(InMemoryProvider::new()),
                )
                .await
                .unwrap(),
            )
        } else {
            Arc::new(InMemoryMemoryStore::default())
        };
        let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let memory = Arc::new(QmdMemory::new_with_workspace(docs.clone(), WORKSPACE));
        memory.set_store(store.clone()).await;
        let state = WorkspaceState::new_minimal(
            WORKSPACE.into(),
            root.path().to_path_buf(),
            memory,
            store.clone(),
        )
        .await;
        let app = Router::new()
            .route("/v1/memories", post(v1_memories_add))
            .route("/v1/memories/{id}", axum::routing::get(v1_memories_get))
            .layer(Extension(WorkspaceContext {
                workspace_id: WORKSPACE.into(),
                workspace: Arc::new(state),
            }));
        Self {
            app,
            store,
            docs,
            _root: root,
        }
    }

    async fn request(&self, uri: &str, payload: Option<Value>) -> Value {
        let request = Request::builder()
            .uri(uri)
            .method(if payload.is_some() { "POST" } else { "GET" })
            .header("content-type", "application/json")
            .body(Body::from(
                payload.map(|value| value.to_string()).unwrap_or_default(),
            ))
            .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        // These legacy handlers document HTTP 200 even for API errors.
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn add(&self, content: &str, index: u32) -> Value {
        self.add_at(PATH, content, index).await
    }

    async fn add_at(&self, path: &str, content: &str, index: u32) -> Value {
        self.request(
            "/v1/memories",
            Some(json!({
                "path": path,
                "text": content,
                "metadata": {"index": index, "clearance": "unclassified"}
            })),
        )
        .await
    }
}

#[tokio::test]
#[serial_test::serial]
async fn http_canonical_metadata_change_increments_persisted_revision() {
    xavier::isolate_test_process!();
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = Fixture::with_vec_store(true).await;
        let path = "features/integrity/http-revision";
        let first = fixture.add_at(path, CONTENT, 0).await;
        assert_eq!(first["status"], "ok");
        let id = first["id"].as_str().expect("canonical add returns an id");
        let initial = fixture.store.get(WORKSPACE, id).await.unwrap().unwrap();
        assert_eq!(initial.revision, 1);

        let retry = fixture.add_at(path, CONTENT, 0).await;
        assert_eq!(retry["status"], "ok");
        assert_eq!(retry["id"], id);
        assert_eq!(
            fixture
                .store
                .get(WORKSPACE, id)
                .await
                .unwrap()
                .unwrap()
                .revision,
            1,
            "identical retry must not create a revision"
        );

        let changed = fixture.add_at(path, CONTENT, 1).await;
        assert_eq!(changed["status"], "ok");
        assert_eq!(changed["id"], id);
        let persisted = fixture.store.get(WORKSPACE, id).await.unwrap().unwrap();
        assert_eq!(persisted.revision, 2);
        assert_eq!(persisted.metadata["index"], 1);
        assert_eq!(fixture.store.list(WORKSPACE).await.unwrap().len(), 1);

        // Force GET to read the persisted row rather than the write cache,
        // whose document metadata does not yet contain the store revision.
        fixture.docs.write().await.clear();
        let resolved = fixture.request(&format!("/v1/memories/{id}"), None).await;
        assert_eq!(resolved["status"], "ok");
        assert_eq!(resolved["memory"]["id"], id);
        assert_eq!(resolved["memory"]["memory"], CONTENT);
        assert_eq!(resolved["memory"]["metadata"]["index"], 1);
        assert_eq!(resolved["memory"]["metadata"]["revision"], 2);
    })
    .await
    .expect("canonical HTTP revision contract must finish within 20 seconds");
}

#[tokio::test]
#[serial_test::serial]
async fn http_add_rejects_corrupt_content_without_persisting() {
    xavier::isolate_test_process!();
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = Fixture::new().await;
        for content in [
            "The audit contains an embedded \u{0} control character.",
            "La全区 prioritario no es agregar features: es dejar de mentir.",
        ] {
            let response = fixture.add(content, 0).await;
            assert_eq!(response["status"], "error");
            assert_eq!(response["code"], "INTERNAL_ERROR");
            assert!(response["message"]
                .as_str()
                .expect("error message")
                .starts_with("Content validation failed:"));
            assert!(response.get("id").is_none());
            assert!(fixture.store.list(WORKSPACE).await.unwrap().is_empty());
        }
    })
    .await
    .expect("HTTP rejection contract must finish within 20 seconds");
}

#[tokio::test]
#[serial_test::serial]
async fn http_add_retry_preserves_id_and_metadata_change_appends() {
    xavier::isolate_test_process!();
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = Fixture::new().await;
        let first = fixture.add(CONTENT, 0).await;
        assert_eq!(first["status"], "ok");
        let first_id = first["id"].as_str().expect("add returns an id");
        assert!(!first_id.is_empty());
        let retry = fixture.add(CONTENT, 0).await;
        assert_eq!(retry["status"], "ok");
        assert_eq!(retry["id"], first_id);
        assert_eq!(fixture.store.list(WORKSPACE).await.unwrap().len(), 1);

        // Non-canonical event paths append distinct records when caller
        // metadata changes; they do not revise the earlier event in place.
        let changed = fixture.add(CONTENT, 1).await;
        assert_eq!(changed["status"], "ok");
        let changed_id = changed["id"].as_str().expect("new event returns an id");
        assert_ne!(first_id, changed_id);
        let records = fixture.store.list(WORKSPACE).await.unwrap();
        assert_eq!(records.len(), 2);
        for (id, index) in [(first_id, 0), (changed_id, 1)] {
            let resolved = fixture.request(&format!("/v1/memories/{id}"), None).await;
            assert_eq!(resolved["status"], "ok");
            assert_eq!(resolved["memory"]["id"], id);
            assert_eq!(resolved["memory"]["memory"], CONTENT);
            assert_eq!(resolved["memory"]["metadata"]["index"], index);
            assert_eq!(resolved["memory"]["metadata"]["integrity"], "verified");
            let record = records.iter().find(|record| record.id == id).unwrap();
            assert_eq!(record.revisions.len(), 1);
            assert_eq!(record.revisions[0].metadata["index"], index);
        }
        let retry_after_append = fixture.add(CONTENT, 0).await;
        assert_eq!(retry_after_append["id"], first_id);
        assert_eq!(fixture.store.list(WORKSPACE).await.unwrap().len(), 2);
    })
    .await
    .expect("HTTP write contract must finish within 20 seconds");
}
