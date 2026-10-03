//! `/v1/training/jobs` HTTP API. Mounted inside the authenticated
//! `protected_routes` group in `cli/server.rs`, like `training_routes`.

use super::backend::{
    job_dir, log_path, BundleManifest, ComputeBackend, JobPhase, LocalBackend,
    ManualNotebookBackend, RemoteRef,
};
use super::jobs::{JobRepository, JobStatus, JobUpdate, NewJob, TrainingJob};
use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub const DEFAULT_BASE_MODEL: &str = "Qwen/Qwen2.5-0.5B-Instruct";
pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const MAX_REPORT_BYTES: u64 = 16 * 1024 * 1024;
const LOG_TAIL_BYTES: u64 = 256 * 1024;
const POLL_INTERVAL_MS: u64 = 500;

#[derive(Debug, Clone)]
pub struct TrainingJobsConfig {
    /// Holds `training_jobs.sqlite3` and `jobs/<id>/`.
    pub data_dir: PathBuf,
    /// Where `bundle_id`s resolve (`<bundles_dir>/<bundle_id>`).
    pub bundles_dir: PathBuf,
    /// Trainer program + leading args.
    pub command: Vec<String>,
    pub max_artifact_bytes: u64,
}

impl TrainingJobsConfig {
    /// `XAVIER_PYTHON` (default `python3`), `XAVIER_TRAIN_SCRIPT`
    /// (default `scripts/training/train_expert.py`),
    /// `XAVIER_TRAIN_MAX_ARTIFACT_BYTES` (default 4 GiB).
    pub fn from_env(data_dir: PathBuf, bundles_dir: PathBuf) -> Self {
        let python = std::env::var("XAVIER_PYTHON").unwrap_or_else(|_| "python3".into());
        let script = std::env::var("XAVIER_TRAIN_SCRIPT")
            .unwrap_or_else(|_| "scripts/training/train_expert.py".into());
        let max = std::env::var("XAVIER_TRAIN_MAX_ARTIFACT_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_ARTIFACT_BYTES);
        Self {
            data_dir,
            bundles_dir,
            command: vec![python, script],
            max_artifact_bytes: max,
        }
    }
}

pub struct JobsState {
    repo: JobRepository,
    local: Arc<dyn ComputeBackend>,
    notebook: Arc<dyn ComputeBackend>,
    jobs_root: PathBuf,
    bundles_dir: PathBuf,
    max_artifact_bytes: u64,
}

pub fn router(cfg: TrainingJobsConfig) -> Result<Router> {
    let jobs_root = cfg.data_dir.join("jobs");
    std::fs::create_dir_all(&jobs_root)?;
    let repo = JobRepository::open(&cfg.data_dir.join("training_jobs.sqlite3"))?;
    Ok(router_with(JobsState {
        repo,
        local: Arc::new(LocalBackend::new(cfg.command.clone(), jobs_root.clone())),
        notebook: Arc::new(ManualNotebookBackend::new(cfg.command, jobs_root.clone())),
        jobs_root,
        bundles_dir: cfg.bundles_dir,
        max_artifact_bytes: cfg.max_artifact_bytes,
    }))
}

fn router_with(state: JobsState) -> Router {
    Router::new()
        .route("/v1/training/jobs", post(create_job).get(list_jobs))
        .route("/v1/training/jobs/{id}", get(get_job))
        .route("/v1/training/jobs/{id}/cancel", post(cancel_job))
        .route("/v1/training/jobs/{id}/logs", get(job_logs))
        .route("/v1/training/jobs/{id}/artifacts", post(upload_artifact))
        .with_state(Arc::new(state))
}

type St = State<Arc<JobsState>>;

fn err(code: StatusCode, msg: impl ToString) -> Response {
    (code, Json(json!({ "error": msg.to_string() }))).into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    err(StatusCode::INTERNAL_SERVER_ERROR, e)
}

#[derive(Debug, Deserialize)]
pub struct CreateJobRequest {
    pub bundle_path: Option<String>,
    pub bundle_id: Option<String>,
    #[serde(default = "default_backend")]
    pub backend: String,
    pub base_model: Option<String>,
    pub domain: String,
}

fn default_backend() -> String {
    "local".into()
}

fn bundle_hash(dir: &std::path::Path) -> std::io::Result<String> {
    let mut h = Sha256::new();
    for f in ["bundle_manifest.json", "train.jsonl", "eval.jsonl"] {
        h.update(f.as_bytes());
        h.update(std::fs::read(dir.join(f))?);
    }
    Ok(crate::crypto::hex_encode(h.finalize()))
}

async fn create_job(State(st): St, Json(req): Json<CreateJobRequest>) -> Response {
    let dir = match (&req.bundle_path, &req.bundle_id) {
        (Some(p), None) => PathBuf::from(p),
        (None, Some(id)) => {
            let ok = !id.is_empty()
                && id != "."
                && id != ".."
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            if !ok {
                return err(StatusCode::BAD_REQUEST, "invalid bundle_id");
            }
            st.bundles_dir.join(id)
        }
        _ => {
            return err(
                StatusCode::BAD_REQUEST,
                "give exactly one of bundle_path, bundle_id",
            )
        }
    };
    let dir = match dir.canonicalize() {
        Ok(d) if d.is_dir() => d,
        _ => return err(StatusCode::BAD_REQUEST, "bundle directory not found"),
    };
    let backend: Arc<dyn ComputeBackend> = match req.backend.as_str() {
        "local" => st.local.clone(),
        "notebook" => st.notebook.clone(),
        other => {
            return err(
                StatusCode::BAD_REQUEST,
                format!("unknown backend {other:?}"),
            )
        }
    };
    let manifest = match BundleManifest::load(&dir) {
        Ok(m) => m,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("bad bundle: {e:#}")),
    };
    if !backend.accepts(&manifest) {
        return err(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!(
                "backend {} refuses this bundle (privacy/audit gate)",
                backend.name()
            ),
        );
    }
    let hash = match bundle_hash(&dir) {
        Ok(h) => h,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("bad bundle: {e}")),
    };
    let job = match st.repo.create(NewJob {
        owner: "admin".into(),
        domain: req.domain,
        bundle_path: dir.to_string_lossy().into_owned(),
        bundle_hash: hash,
        backend: req.backend,
        base_model: req.base_model.unwrap_or_else(|| DEFAULT_BASE_MODEL.into()),
    }) {
        Ok(j) => j,
        Err(e) => return internal(e),
    };
    tokio::spawn(drive(st.clone(), backend, job.clone()));
    (StatusCode::ACCEPTED, Json(job)).into_response()
}

fn fail(st: &JobsState, id: &str, msg: String) {
    let _ = st.repo.update(
        id,
        JobUpdate {
            status: Some(JobStatus::Failed),
            error: Some(msg),
            ..Default::default()
        },
    );
}

/// Submits the job and follows it to a terminal (or awaiting) state.
async fn drive(st: Arc<JobsState>, backend: Arc<dyn ComputeBackend>, job: TrainingJob) {
    let r = RemoteRef(job.id.clone());
    let logs = log_path(&st.jobs_root, &job.id)
        .to_string_lossy()
        .into_owned();
    let remote = match backend.submit(&job).await {
        Ok(_) => r.clone(),
        Err(e) => return fail(&st, &job.id, format!("{e:#}")),
    };
    let first = if backend.name() == "local" {
        JobStatus::Running
    } else {
        JobStatus::AwaitingArtifact
    };
    let applied = st
        .repo
        .update(
            &job.id,
            JobUpdate {
                status: Some(first),
                logs_path: Some(logs),
                ..Default::default()
            },
        )
        .unwrap_or(false);
    if !applied {
        // cancelled while submitting
        let _ = backend.cancel(&remote).await;
        return;
    }
    if first == JobStatus::AwaitingArtifact {
        return; // the human finishes it via POST .../artifacts
    }
    loop {
        match backend.poll(&remote).await {
            Ok(JobPhase::Running) | Ok(JobPhase::AwaitingArtifact) => {
                tokio::time::sleep(std::time::Duration::from_millis(POLL_INTERVAL_MS)).await
            }
            Ok(JobPhase::Succeeded) => {
                let dst = job_dir(&st.jobs_root, &job.id).join("artifacts");
                match backend.fetch_artifacts(&remote, &dst).await {
                    Ok(a) => finish_ok(&st, &job.id, &a.gguf, a.report.as_deref()),
                    Err(e) => fail(&st, &job.id, format!("{e:#}")),
                }
                return;
            }
            Ok(JobPhase::Failed(m)) => return fail(&st, &job.id, m),
            Ok(JobPhase::Cancelled) => return,
            Err(e) => return fail(&st, &job.id, format!("{e:#}")),
        }
    }
}

fn finish_ok(st: &JobsState, id: &str, gguf: &std::path::Path, report: Option<&std::path::Path>) {
    let metrics = report
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| v.to_string());
    let _ = st.repo.update(
        id,
        JobUpdate {
            status: Some(JobStatus::Succeeded),
            artifact_path: Some(gguf.to_string_lossy().into_owned()),
            metrics_json: metrics,
            ..Default::default()
        },
    );
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

async fn list_jobs(State(st): St, Query(q): Query<ListQuery>) -> Response {
    match st.repo.list(q.limit.unwrap_or(100).min(1000)) {
        Ok(jobs) => Json(json!({ "jobs": jobs })).into_response(),
        Err(e) => internal(e),
    }
}

#[allow(clippy::result_large_err)]
fn load(st: &JobsState, id: &str) -> Result<TrainingJob, Response> {
    match st.repo.get(id) {
        Ok(Some(j)) => Ok(j),
        Ok(None) => Err(err(StatusCode::NOT_FOUND, "job not found")),
        Err(e) => Err(internal(e)),
    }
}

async fn get_job(State(st): St, Path(id): Path<String>) -> Response {
    match load(&st, &id) {
        Ok(j) => Json(j).into_response(),
        Err(r) => r,
    }
}

async fn cancel_job(State(st): St, Path(id): Path<String>) -> Response {
    let job = match load(&st, &id) {
        Ok(j) => j,
        Err(r) => return r,
    };
    match st.repo.cancel(&id) {
        Ok(true) => {
            let b = if job.backend == "local" {
                &st.local
            } else {
                &st.notebook
            };
            let _ = b.cancel(&RemoteRef(id.clone())).await;
            match load(&st, &id) {
                Ok(j) => Json(j).into_response(),
                Err(r) => r,
            }
        }
        Ok(false) => err(StatusCode::CONFLICT, "job already finished"),
        Err(e) => internal(e),
    }
}

async fn job_logs(State(st): St, Path(id): Path<String>) -> Response {
    if let Err(r) = load(&st, &id) {
        return r;
    }
    let path = log_path(&st.jobs_root, &id);
    let text = (|| -> std::io::Result<String> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&path)?;
        let len = f.metadata()?.len();
        f.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL_BYTES)))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    })()
    .unwrap_or_default();
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

#[derive(Debug, Deserialize)]
struct UploadQuery {
    /// `gguf` (default) or `report`.
    kind: Option<String>,
}

/// Raw-body upload for the manual flow: `POST .../artifacts?kind=gguf` (the
/// model, magic bytes `GGUF` verified) and `?kind=report` (`train_report.json`).
async fn upload_artifact(
    State(st): St,
    Path(id): Path<String>,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> Response {
    let job = match load(&st, &id) {
        Ok(j) => j,
        Err(r) => return r,
    };
    if job.backend != "notebook" || job.status != JobStatus::AwaitingArtifact {
        return err(
            StatusCode::CONFLICT,
            "job is not a notebook job awaiting artifacts",
        );
    }
    let kind = q.kind.unwrap_or_else(|| "gguf".into());
    let (is_gguf, limit, name) = match kind.as_str() {
        "gguf" => (true, st.max_artifact_bytes, "model.gguf"),
        "report" => (false, MAX_REPORT_BYTES, "train_report.json"),
        _ => return err(StatusCode::BAD_REQUEST, "kind must be gguf or report"),
    };
    let dir = job_dir(&st.jobs_root, &id).join("artifacts");
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return internal(e);
    }
    let part = dir.join(format!(".upload-{}.part", uuid::Uuid::new_v4()));
    let result = receive(body, &part, is_gguf, limit).await;
    let written = match result {
        Ok(n) => n,
        Err((code, msg)) => {
            let _ = tokio::fs::remove_file(&part).await;
            return err(code, msg);
        }
    };
    if !is_gguf {
        let ok = tokio::fs::read(&part)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .is_some();
        if !ok {
            let _ = tokio::fs::remove_file(&part).await;
            return err(
                StatusCode::UNPROCESSABLE_ENTITY,
                "train_report.json is not valid JSON",
            );
        }
    }
    if let Err(e) = tokio::fs::rename(&part, dir.join(name)).await {
        return internal(e);
    }
    let mut status = job.status;
    if let Ok(JobPhase::Succeeded) = st.notebook.poll(&RemoteRef(id.clone())).await {
        finish_ok(
            &st,
            &id,
            &dir.join("model.gguf"),
            Some(&dir.join("train_report.json")),
        );
        status = JobStatus::Succeeded;
    }
    Json(json!({ "kind": kind, "bytes": written, "status": status })).into_response()
}

async fn receive(
    body: Body,
    part: &std::path::Path,
    check_magic: bool,
    limit: u64,
) -> std::result::Result<u64, (StatusCode, String)> {
    let io = |e: std::io::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    let mut file = tokio::fs::File::create(part).await.map_err(io)?;
    let mut stream = body.into_data_stream();
    let mut total: u64 = 0;
    let mut head: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        total += chunk.len() as u64;
        if total > limit {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("upload exceeds {limit} bytes"),
            ));
        }
        if check_magic && head.len() < 4 {
            head.extend_from_slice(&chunk[..chunk.len().min(4 - head.len())]);
            if head.len() == 4 && head != b"GGUF" {
                return Err((
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "not a GGUF file (bad magic bytes)".into(),
                ));
            }
        }
        file.write_all(&chunk).await.map_err(io)?;
    }
    file.flush().await.map_err(io)?;
    if check_magic && head != b"GGUF" {
        return Err((
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "not a GGUF file (bad magic bytes)".into(),
        ));
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::super::backend::test_support::*;
    use super::*;
    use axum::http::Request;
    use tower::ServiceExt;

    struct Fixture {
        _t: tempfile::TempDir,
        app: Router,
        root: PathBuf,
        good: PathBuf,
    }

    fn fixture(script_body: &str, max: u64) -> Fixture {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("data");
        let cmd = write_script(t.path(), script_body);
        let jobs = root.join("jobs");
        std::fs::create_dir_all(&jobs).unwrap();
        let good = write_bundle(
            &t.path().join("g"),
            r#"{"privacy_level":"P3","local_only":false}"#,
            Some(r#"{"passed":true}"#),
        );
        let app = router_with(JobsState {
            repo: JobRepository::open(&root.join("t.sqlite3")).unwrap(),
            local: Arc::new(LocalBackend::new(cmd.clone(), jobs.clone())),
            notebook: Arc::new(ManualNotebookBackend::new(cmd, jobs.clone())),
            jobs_root: jobs,
            bundles_dir: t.path().join("bundles"),
            max_artifact_bytes: max,
        });
        Fixture {
            _t: t,
            app,
            root,
            good,
        }
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        body: Vec<u8>,
    ) -> (StatusCode, serde_json::Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let code = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            code,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    fn create_body(bundle: &std::path::Path, backend: &str) -> Vec<u8> {
        json!({"bundle_path": bundle, "backend": backend, "domain": "legal"})
            .to_string()
            .into_bytes()
    }

    async fn wait_status(app: &Router, id: &str, want: &str) -> serde_json::Value {
        for _ in 0..200 {
            let (_, j) = call(app, "GET", &format!("/v1/training/jobs/{id}"), vec![]).await;
            if j["status"] == want {
                return j;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("job {id} never reached {want}");
    }

    #[tokio::test]
    async fn lifecycle_success() {
        let f = fixture(
            "echo hello-log; printf GGUF0000 > \"$OUT/m.gguf\"; echo '{\"loss\":0.1}' > \"$OUT/train_report.json\"",
            1 << 20,
        );
        let (code, j) = call(
            &f.app,
            "POST",
            "/v1/training/jobs",
            create_body(&f.good, "local"),
        )
        .await;
        assert_eq!(code, StatusCode::ACCEPTED);
        let id = j["id"].as_str().unwrap().to_string();
        let done = wait_status(&f.app, &id, "succeeded").await;
        assert!(done["artifact_path"].as_str().unwrap().ends_with("m.gguf"));
        assert!(done["metrics_json"].as_str().unwrap().contains("loss"));
        let req = Request::builder()
            .uri(format!("/v1/training/jobs/{id}/logs"))
            .body(Body::empty())
            .unwrap();
        let res = f.app.clone().oneshot(req).await.unwrap();
        let b = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&b).contains("hello-log"));
        let (_, l) = call(&f.app, "GET", "/v1/training/jobs", vec![]).await;
        assert_eq!(l["jobs"].as_array().unwrap().len(), 1);
        let (c, _) = call(
            &f.app,
            "POST",
            &format!("/v1/training/jobs/{id}/cancel"),
            vec![],
        )
        .await;
        assert_eq!(c, StatusCode::CONFLICT);
        let (c, _) = call(&f.app, "GET", "/v1/training/jobs/nope", vec![]).await;
        assert_eq!(c, StatusCode::NOT_FOUND);
        assert!(f
            .root
            .join("jobs")
            .join(&id)
            .join("artifacts")
            .join("m.gguf")
            .is_file());
    }

    #[tokio::test]
    async fn lifecycle_failure() {
        let f = fixture("echo boom >&2; exit 2", 1 << 20);
        let (_, j) = call(
            &f.app,
            "POST",
            "/v1/training/jobs",
            create_body(&f.good, "local"),
        )
        .await;
        let id = j["id"].as_str().unwrap();
        let done = wait_status(&f.app, id, "failed").await;
        assert!(done["error"].as_str().unwrap().contains("exit 2"));
    }

    #[tokio::test]
    async fn cancel_running_job() {
        let f = fixture("sleep 30", 1 << 20);
        let (_, j) = call(
            &f.app,
            "POST",
            "/v1/training/jobs",
            create_body(&f.good, "local"),
        )
        .await;
        let id = j["id"].as_str().unwrap().to_string();
        wait_status(&f.app, &id, "running").await;
        let (c, j) = call(
            &f.app,
            "POST",
            &format!("/v1/training/jobs/{id}/cancel"),
            vec![],
        )
        .await;
        assert_eq!(c, StatusCode::OK);
        assert_eq!(j["status"], "cancelled");
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        let (_, j) = call(&f.app, "GET", &format!("/v1/training/jobs/{id}"), vec![]).await;
        assert_eq!(j["status"], "cancelled"); // monitor must not overwrite
    }

    #[tokio::test]
    async fn notebook_refusal_and_manual_upload() {
        let f = fixture("echo nb > \"$OUT/x.ipynb\"", 64);
        let bad = write_bundle(
            &f._t.path().join("b"),
            r#"{"privacy_level":"P4","local_only":true}"#,
            None,
        );
        let (c, _) = call(
            &f.app,
            "POST",
            "/v1/training/jobs",
            create_body(&bad, "notebook"),
        )
        .await;
        assert_eq!(c, StatusCode::UNPROCESSABLE_ENTITY);
        let (_, l) = call(&f.app, "GET", "/v1/training/jobs", vec![]).await;
        assert!(l["jobs"].as_array().unwrap().is_empty());

        let (c, j) = call(
            &f.app,
            "POST",
            "/v1/training/jobs",
            create_body(&f.good, "notebook"),
        )
        .await;
        assert_eq!(c, StatusCode::ACCEPTED);
        let id = j["id"].as_str().unwrap().to_string();
        wait_status(&f.app, &id, "awaiting_artifact").await;
        let up = format!("/v1/training/jobs/{id}/artifacts");

        let (c, _) = call(&f.app, "POST", &up, b"PK\x03\x04 not a gguf".to_vec()).await;
        assert_eq!(c, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let (c, _) = call(
            &f.app,
            "POST",
            &up,
            [b"GGUF".as_slice(), &[0u8; 100]].concat(),
        )
        .await;
        assert_eq!(c, StatusCode::PAYLOAD_TOO_LARGE);
        let (c, _) = call(
            &f.app,
            "POST",
            &format!("{up}?kind=report"),
            b"not json".to_vec(),
        )
        .await;
        assert_eq!(c, StatusCode::UNPROCESSABLE_ENTITY);

        let (c, r) = call(&f.app, "POST", &up, b"GGUF-model".to_vec()).await;
        assert_eq!(c, StatusCode::OK);
        assert_eq!(r["status"], "awaiting_artifact");
        let (c, r) = call(
            &f.app,
            "POST",
            &format!("{up}?kind=report"),
            br#"{"ok":1}"#.to_vec(),
        )
        .await;
        assert_eq!(c, StatusCode::OK);
        assert_eq!(r["status"], "succeeded");
        let (_, j) = call(&f.app, "GET", &format!("/v1/training/jobs/{id}"), vec![]).await;
        assert!(j["artifact_path"].as_str().unwrap().ends_with("model.gguf"));
    }
}
