//! feat-explain-render-route: On-demand code explanations.
//! Explain route renders with an injected model and resolves the nested repo.

#![allow(clippy::needless_return)]

xavier::isolate_test_process!();

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};

use std::sync::{Arc, Mutex};
use tower::ServiceExt;

use xavier::explain::context::EvidenceSource;
use xavier::explain::history::GitHistorySource;
use xavier::explain::render::ExplanationModel;
use xavier::server::explain_route::{router, ExplainDeps};

/// Dummy model that records the user prompt it received and returns a fixed answer.
struct RecordingModel {
    answer: String,
    prompt: Arc<Mutex<String>>,
}

impl ExplanationModel for RecordingModel {
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        user: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        let mut guard = self.prompt.lock().unwrap();
        *guard = user.to_string();
        let answer = self.answer.clone();
        Box::pin(async move { Ok(answer) })
    }
}

/// Dummy model that returns an error.
struct ErrorModel;

impl ExplanationModel for ErrorModel {
    fn complete<'a>(
        &'a self,
        _system: &'a str,
        _user: &'a str,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move { anyhow::bail!("Model failed intentionally") })
    }
}

fn setup_repo_fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();

    let mut git_init = std::process::Command::new("git");
    git_init.arg("init").arg("-q").current_dir(dir.path());
    for (k, _) in std::env::vars() {
        if k.starts_with("GIT_") {
            git_init.env_remove(k);
        }
    }
    let status = git_init.status().unwrap();
    assert!(status.success());

    let nested_dir = dir.path().join("nested").join("repo");
    std::fs::create_dir_all(&nested_dir).unwrap();

    let mut git_init_nested = std::process::Command::new("git");
    git_init_nested
        .arg("init")
        .arg("-q")
        .current_dir(&nested_dir);
    for (k, _) in std::env::vars() {
        if k.starts_with("GIT_") {
            git_init_nested.env_remove(k);
        }
    }
    let status = git_init_nested.status().unwrap();
    assert!(status.success());

    let src_dir = nested_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();

    let mut git_add = std::process::Command::new("git");
    git_add.arg("add").arg(".").current_dir(&nested_dir);
    for (k, _) in std::env::vars() {
        if k.starts_with("GIT_") {
            git_add.env_remove(k);
        }
    }
    let status = git_add.status().unwrap();
    assert!(status.success());

    let mut git_commit = std::process::Command::new("git");
    git_commit
        .arg("-c")
        .arg("user.name=Test User")
        .arg("-c")
        .arg("user.email=test@example.com")
        .arg("commit")
        .arg("-q")
        .arg("-m")
        .arg("feat: initial commit for lib")
        .current_dir(&nested_dir);
    for (k, _) in std::env::vars() {
        if k.starts_with("GIT_") {
            git_commit.env_remove(k);
        }
    }
    let status = git_commit.status().unwrap();
    assert!(status.success());

    dir
}

#[tokio::test]
async fn explain_resolves_nested_repo() {
    let dir = setup_repo_fixture();
    let prompt_ref = Arc::new(Mutex::new(String::new()));

    let deps = ExplainDeps {
        repo_root: dir.path().to_path_buf(),
        sources: Arc::new(|| vec![Box::new(GitHistorySource) as Box<dyn EvidenceSource + Send>]),
        model: Arc::new(RecordingModel {
            answer: "Fixed dummy answer".to_string(),
            prompt: prompt_ref.clone(),
        }),
    };

    let app = router(deps);

    let req_body = json!({
        "file": "nested/repo/src/lib.rs",
        "start_line": 1,
        "end_line": 3
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/explain")
        .header("content-type", "application/json")
        .body(Body::from(req_body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let resp_json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

    assert_eq!(status, StatusCode::OK);
    assert_eq!(resp_json["plain_text"], "Fixed dummy answer");
    assert_eq!(resp_json["repo"], "nested/repo");

    let prompt_content = prompt_ref.lock().unwrap().clone();
    assert!(
        prompt_content.contains("feat: initial commit for lib"),
        "Prompt did not contain the commit subject. Prompt was: {}",
        prompt_content
    );
}

#[tokio::test]
async fn explain_maps_model_failure_to_502() {
    let dir = setup_repo_fixture();

    let deps = ExplainDeps {
        repo_root: dir.path().to_path_buf(),
        sources: Arc::new(|| vec![Box::new(GitHistorySource) as Box<dyn EvidenceSource + Send>]),
        model: Arc::new(ErrorModel),
    };

    let app = router(deps);

    let req_body = json!({
        "file": "nested/repo/src/lib.rs",
        "start_line": 1,
        "end_line": 3
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/explain")
        .header("content-type", "application/json")
        .body(Body::from(req_body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let resp_json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        resp_json["error"]
            .as_str()
            .unwrap()
            .contains("Model failed intentionally"),
        "Unexpected error message: {:?}",
        resp_json["error"]
    );
}
