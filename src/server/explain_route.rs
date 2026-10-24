//! `POST /v1/explain`: explains a code fragment as generative UI.
//!
//! Path: src/server/explain_route.rs
//!
//! Validates the request, assembles evidence for the lines, asks the
//! explanation model for an answer and renders it. Not mounted by the server
//! yet; wiring happens in `cli/server.rs` once the provider rework lands.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::explain::{
    assemble, explain, Audience, EvidenceSource, ExplainTarget, Explanation, ExplanationModel,
};

/// Largest line range one explanation may cover.
const MAX_RANGE_LINES: u32 = 400;
/// Evidence budget used when the request does not set one.
const DEFAULT_BUDGET_CHARS: usize = 12_000;
/// Largest evidence budget a request may ask for.
const MAX_BUDGET_CHARS: usize = 64_000;

/// Body of `POST /v1/explain`. Lines are 1-based and inclusive.
#[derive(Debug, Clone, Deserialize)]
pub struct ExplainRequest {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol: Option<String>,
    pub audience: Option<Audience>,
    pub budget_chars: Option<usize>,
}

/// Shared state for the route.
#[derive(Clone)]
pub struct ExplainDeps {
    pub repo_root: PathBuf,
    /// Builds a fresh set of evidence sources for each request.
    pub sources: Arc<dyn Fn() -> Vec<Box<dyn EvidenceSource + Send>> + Send + Sync>,
    pub model: Arc<dyn ExplanationModel>,
}

/// An error response with its status code and a JSON `error` field.
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn bad_request(message: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.into())
}

/// Checks the line range and budget without touching the filesystem.
fn validate(req: &ExplainRequest) -> Result<(), ApiError> {
    if req.start_line == 0 || req.start_line > req.end_line {
        return Err(bad_request(format!(
            "invalid line range {}-{}",
            req.start_line, req.end_line
        )));
    }
    if req.end_line - req.start_line + 1 > MAX_RANGE_LINES {
        return Err(bad_request(format!(
            "line range exceeds {MAX_RANGE_LINES} lines"
        )));
    }
    if req
        .budget_chars
        .is_some_and(|budget| budget > MAX_BUDGET_CHARS)
    {
        return Err(bad_request(format!(
            "budget_chars exceeds {MAX_BUDGET_CHARS}"
        )));
    }
    Ok(())
}

/// Checks that `file` is a regular file under `repo_root`. Symlinks are
/// resolved before the containment check, so a link pointing outside the
/// repo is rejected.
fn resolve_inside(repo_root: &Path, file: &str) -> Result<(), ApiError> {
    let relative = Path::new(file);
    let is_relative = !file.is_empty()
        && relative
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if !is_relative {
        return Err(bad_request(format!(
            "file must be a relative path inside the repo: {file}"
        )));
    }

    let root = repo_root.canonicalize().map_err(|err| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("repo root unavailable: {err}"),
        )
    })?;
    let resolved = repo_root
        .join(relative)
        .canonicalize()
        .map_err(|err| bad_request(format!("cannot resolve {file}: {err}")))?;
    if !resolved.starts_with(&root) {
        return Err(bad_request(format!("{file} resolves outside the repo")));
    }
    if !resolved.is_file() {
        return Err(bad_request(format!("{file} is not a file")));
    }
    Ok(())
}

/// POST `/v1/explain`: returns the rendered explanation for a code fragment.
async fn explain_handler(
    State(deps): State<ExplainDeps>,
    Json(req): Json<ExplainRequest>,
) -> Result<Json<Explanation>, ApiError> {
    validate(&req)?;
    resolve_inside(&deps.repo_root, &req.file)?;

    let audience = req.audience.unwrap_or_default();
    let budget = req.budget_chars.unwrap_or(DEFAULT_BUDGET_CHARS);
    let target = ExplainTarget {
        repo_root: deps.repo_root.clone(),
        file: PathBuf::from(&req.file),
        start_line: req.start_line,
        end_line: req.end_line,
        symbol: req.symbol.clone(),
    };

    let sources = Arc::clone(&deps.sources);
    let context = tokio::task::spawn_blocking(move || {
        let boxed = sources();
        let refs: Vec<&dyn EvidenceSource> =
            boxed.iter().map(|s| &**s as &dyn EvidenceSource).collect();
        assemble(&target, &refs, budget)
    })
    .await
    .map_err(|err| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("evidence task failed: {err}"),
        )
    })?;

    let explanation = explain(&context, deps.model.as_ref(), audience)
        .await
        .map_err(|err| ApiError(StatusCode::BAD_GATEWAY, format!("{err:#}")))?;
    Ok(Json(explanation))
}

/// Router for `POST /v1/explain`. Not mounted by the server yet.
pub fn router(deps: ExplainDeps) -> Router {
    Router::new()
        .route("/v1/explain", post(explain_handler))
        .with_state(deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explain::{Evidence, EvidenceKind};
    use anyhow::anyhow;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use futures_util::future::BoxFuture;
    use serde_json::Value;
    use tower::util::ServiceExt;

    /// Always contributes one memory item.
    struct StaticSource;

    impl EvidenceSource for StaticSource {
        fn name(&self) -> &str {
            "static"
        }

        fn collect(&self, _target: &ExplainTarget, _limit: usize) -> anyhow::Result<Vec<Evidence>> {
            Ok(vec![Evidence {
                kind: EvidenceKind::Memory,
                source_ref: "mem-1".to_string(),
                text: "remembered note".to_string(),
                score: 0.5,
            }])
        }
    }

    /// Answers with a fixed text, or fails with `upstream 503` when `None`.
    struct StubModel(Option<String>);

    impl ExplanationModel for StubModel {
        fn complete<'a>(
            &'a self,
            _system: &'a str,
            _user: &'a str,
        ) -> BoxFuture<'a, anyhow::Result<String>> {
            Box::pin(async move { self.0.clone().ok_or_else(|| anyhow!("upstream 503")) })
        }
    }

    fn deps(repo: &Path, answer: Option<&str>) -> ExplainDeps {
        ExplainDeps {
            repo_root: repo.to_path_buf(),
            sources: Arc::new(|| vec![Box::new(StaticSource) as Box<dyn EvidenceSource + Send>]),
            model: Arc::new(StubModel(answer.map(str::to_string))),
        }
    }

    /// A repo with `src/lib.rs` holding three lines.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn one() {}\nfn two() {}\nfn three() {}\n",
        )
        .unwrap();
        dir
    }

    async fn post_explain(app: Router, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/explain")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn explain_endpoint_returns_explanation_json() {
        let dir = repo();
        let answer = "Adds nothing [src/lib.rs:1-2], see [mem-1] and [ghost-9].";
        let app = router(deps(dir.path(), Some(answer)));

        let (status, json) = post_explain(
            app,
            json!({"file": "src/lib.rs", "start_line": 1, "end_line": 2}),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["cited_refs"], json!(["src/lib.rs:1-2", "mem-1"]));
        assert_eq!(json["unknown_refs"], json!(["ghost-9"]));
        assert!(json["openui_lang"].as_str().is_some_and(|s| !s.is_empty()));
        assert_eq!(json["plain_text"], json!(answer));
        assert!(json["skipped_sources"].is_array());
    }

    #[tokio::test]
    async fn explain_rejects_parent_and_absolute_paths() {
        let dir = repo();
        for file in ["../outside.rs", "/etc/passwd", "src/missing.rs"] {
            let app = router(deps(dir.path(), Some("ok")));
            let (status, _) =
                post_explain(app, json!({"file": file, "start_line": 1, "end_line": 1})).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "file {file}");
        }
    }

    #[tokio::test]
    async fn explain_rejects_inverted_and_oversized_ranges() {
        let dir = repo();
        let cases = [(3, 1), (1, 401)];
        for (start, end) in cases {
            let app = router(deps(dir.path(), Some("ok")));
            let (status, _) = post_explain(
                app,
                json!({"file": "src/lib.rs", "start_line": start, "end_line": end}),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "range {start}-{end}");
        }

        let app = router(deps(dir.path(), Some("ok")));
        let (status, _) = post_explain(
            app,
            json!({"file": "src/lib.rs", "start_line": 1, "end_line": 400}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn explain_rejects_symlink_escaping_the_repo() {
        let dir = repo();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.rs"), "const KEY: u8 = 1;\n").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.rs"),
            dir.path().join("src/escape.rs"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("src/lib.rs"),
            dir.path().join("src/alias.rs"),
        )
        .unwrap();

        let app = router(deps(dir.path(), Some("ok")));
        let (status, json) = post_explain(
            app,
            json!({"file": "src/escape.rs", "start_line": 1, "end_line": 1}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(json["error"]
            .as_str()
            .is_some_and(|e| e.contains("outside the repo")));

        let app = router(deps(dir.path(), Some("ok")));
        let (status, _) = post_explain(
            app,
            json!({"file": "src/alias.rs", "start_line": 1, "end_line": 1}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn explain_maps_model_failure_to_502() {
        let dir = repo();
        let app = router(deps(dir.path(), None));

        let (status, json) = post_explain(
            app,
            json!({"file": "src/lib.rs", "start_line": 1, "end_line": 1}),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert!(json["error"]
            .as_str()
            .is_some_and(|e| e.contains("upstream 503")));
    }
}
