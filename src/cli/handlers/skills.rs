//! CLI handlers for `xavier skills plan|apply|rollback` (D4): the skill controller's only
//! mutation surface. `plan` is the default and persists nothing; `apply` requires an
//! explicit saved `--plan <file>` and rechecks every precondition before any write, only
//! mutating when the caller sets `dry_run: false`; `rollback` restores one journaled
//! transaction, reporting anything edited since apply as a conflict it leaves untouched.
//! The filesystem policy and resolved roots come from the manifest's own declared
//! source/target directories, so no new `XAVIER_SKILL_*` env var is read here.

use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde::Deserialize;
use std::path::PathBuf;
use uuid::Uuid;

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::context::skill_controller::manifest::Manifest;
use xavier::context::skill_controller::plan::{ProjectionPlan, ResolvedRoots};
use xavier::context::skill_controller::planner::build_plan;
use xavier::context::skill_controller::policy::FsPolicy;
use xavier::context::skill_controller::projector::{apply_plan, ApplyReport, EntryOutcome};
use xavier::context::skill_controller::rollback::{
    rollback_transaction, RollbackOutcome, RollbackReport,
};
use xavier::context::skill_controller::state::Journal;

fn default_dry_run() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct SkillsPlanRequest {
    pub manifest_path: String,
}

#[derive(Debug, Deserialize)]
pub struct SkillsApplyRequest {
    pub manifest_path: String,
    pub plan_path: String,
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

#[derive(Debug, Deserialize)]
pub struct SkillsRollbackRequest {
    pub manifest_path: String,
    pub tx_id: String,
    #[serde(default = "default_dry_run")]
    pub dry_run: bool,
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    json_response(status, serde_json::json!({"error": message.into()}))
}

/// Read and validate the manifest at `path`; blocking IO never runs on the async worker.
async fn read_manifest(path: String) -> Result<Manifest, String> {
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read manifest '{path}': {e}"))?;
        Manifest::from_toml(&content).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("manifest read task failed: {e}"))?
}

/// The policy allowlist and resolved roots come straight from the manifest's own declared
/// source/target directories: repository content cannot widen this beyond what the
/// manifest itself names (D5).
fn roots_and_policy(manifest: &Manifest) -> Result<(ResolvedRoots, FsPolicy), String> {
    let sources: Vec<PathBuf> = manifest.sources.values().map(PathBuf::from).collect();
    let targets: Vec<PathBuf> = manifest
        .targets
        .iter()
        .map(|t| PathBuf::from(&t.root))
        .collect();
    let policy = FsPolicy::new(&sources, &targets).map_err(|e| e.to_string())?;
    let roots = ResolvedRoots {
        sources: manifest
            .sources
            .iter()
            .map(|(id, dir)| (id.clone(), PathBuf::from(dir)))
            .collect(),
        targets: manifest
            .targets
            .iter()
            .map(|t| (t.id.clone(), PathBuf::from(&t.root)))
            .collect(),
    };
    Ok((roots, policy))
}

fn entry_outcome_json(outcome: &EntryOutcome) -> serde_json::Value {
    match outcome {
        EntryOutcome::Created => serde_json::json!({"kind": "created"}),
        EntryOutcome::Refreshed => serde_json::json!({"kind": "refreshed"}),
        EntryOutcome::Unchanged => serde_json::json!({"kind": "unchanged"}),
        EntryOutcome::SkippedConflict => serde_json::json!({
            "kind": "conflict",
            "detail": "destination is not controller-owned; remove or rename it, then plan again",
        }),
        EntryOutcome::BlockedSecret(summary) => serde_json::json!({
            "kind": "blocked_secret",
            "detail": format!(
                "publication blocked: {summary}; remove the secret from the source package, then plan again"
            ),
        }),
    }
}

fn render_apply_report(report: &ApplyReport) -> serde_json::Value {
    let applied: Vec<serde_json::Value> = report
        .applied
        .iter()
        .map(|(name, target, outcome)| {
            serde_json::json!({
                "name": name,
                "target": target,
                "outcome": entry_outcome_json(outcome),
            })
        })
        .collect();
    let removed: Vec<serde_json::Value> = report
        .removed
        .iter()
        .map(|(name, target)| serde_json::json!({"name": name, "target": target}))
        .collect();
    serde_json::json!({
        "tx_id": report.tx_id,
        "dry_run": report.dry_run,
        "applied": applied,
        "removed": removed,
    })
}

fn rollback_outcome_json(outcome: &RollbackOutcome) -> serde_json::Value {
    match outcome {
        RollbackOutcome::RestoredLink { to } => {
            serde_json::json!({"kind": "restored_link", "to": to.to_string_lossy()})
        }
        RollbackOutcome::RestoredCopy => serde_json::json!({"kind": "restored_copy"}),
        RollbackOutcome::Removed => serde_json::json!({"kind": "removed"}),
        RollbackOutcome::AlreadyRolledBack => serde_json::json!({"kind": "already_rolled_back"}),
        RollbackOutcome::Conflict(detail) => serde_json::json!({
            "kind": "conflict",
            "detail": format!("{detail}; left untouched, resolve manually then roll back again"),
        }),
    }
}

fn render_rollback_report(report: &RollbackReport) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = report
        .entries
        .iter()
        .map(|(path, outcome)| {
            serde_json::json!({
                "path": path.to_string_lossy(),
                "outcome": rollback_outcome_json(outcome),
            })
        })
        .collect();
    serde_json::json!({"tx_id": report.tx_id, "dry_run": report.dry_run, "entries": entries})
}

/// `plan`: builds and returns a `ProjectionPlan`. Writes nothing on any path (D4).
async fn plan_skills(req: SkillsPlanRequest) -> Response {
    let manifest = match read_manifest(req.manifest_path).await {
        Ok(m) => m,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    let (roots, policy) = match roots_and_policy(&manifest) {
        Ok(v) => v,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    match build_plan(&manifest, &policy, &roots) {
        Ok(plan) => json_response(StatusCode::OK, serde_json::json!({"plan": plan})),
        Err(e) => error_response(StatusCode::CONFLICT, e.to_string()),
    }
}

/// `apply --plan <file>`: fails before touching the journal or disk when no saved plan is
/// readable; only mutates when `dry_run` is explicitly `false` (D4).
async fn apply_skills(req: SkillsApplyRequest, journal: Journal) -> Response {
    if req.plan_path.trim().is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "apply requires --plan <file>; run plan first and save its output",
        );
    }
    let plan_path = req.plan_path.clone();
    let plan: Result<ProjectionPlan, String> = tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&plan_path)
            .map_err(|e| format!("no saved plan at '{plan_path}': {e}"))?;
        serde_json::from_str::<ProjectionPlan>(&content)
            .map_err(|e| format!("saved plan at '{plan_path}' is not valid: {e}"))
    })
    .await
    .map_err(|e| format!("plan read task failed: {e}"))
    .and_then(|inner| inner);
    let plan = match plan {
        Ok(p) => p,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    let manifest = match read_manifest(req.manifest_path).await {
        Ok(m) => m,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    let (roots, policy) = match roots_and_policy(&manifest) {
        Ok(v) => v,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    let tx_id = Uuid::new_v4().to_string();
    let dry_run = req.dry_run;
    let result = tokio::task::spawn_blocking(move || {
        apply_plan(&plan, &manifest, &policy, &roots, &journal, &tx_id, dry_run)
    })
    .await;
    match result {
        Ok(Ok(report)) => json_response(StatusCode::OK, render_apply_report(&report)),
        Ok(Err(e)) => error_response(StatusCode::CONFLICT, e.to_string()),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("apply task failed: {e}"),
        ),
    }
}

/// `rollback`: an entry edited since apply is reported as a conflict and left exactly as
/// it is, never reverted (D14).
async fn rollback_skills(req: SkillsRollbackRequest, journal: Journal) -> Response {
    let manifest = match read_manifest(req.manifest_path).await {
        Ok(m) => m,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    let (_roots, policy) = match roots_and_policy(&manifest) {
        Ok(v) => v,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, e),
    };
    let tx_id = req.tx_id.clone();
    let dry_run = req.dry_run;
    let result = tokio::task::spawn_blocking(move || {
        rollback_transaction(&journal, &policy, &tx_id, dry_run)
    })
    .await;
    match result {
        Ok(Ok(report)) => json_response(StatusCode::OK, render_rollback_report(&report)),
        Ok(Err(e)) => error_response(StatusCode::NOT_FOUND, e.to_string()),
        Err(e) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("rollback task failed: {e}"),
        ),
    }
}

/// `POST` handler for `xavier skills plan`.
pub async fn skills_plan_handler(
    State(_state): State<CliState>,
    Json(payload): Json<SkillsPlanRequest>,
) -> Response {
    plan_skills(payload).await
}

/// `POST` handler for `xavier skills apply --plan <file>`.
pub async fn skills_apply_handler(
    State(_state): State<CliState>,
    Json(payload): Json<SkillsApplyRequest>,
) -> Response {
    let journal = match Journal::from_settings() {
        Ok(j) => j,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    apply_skills(payload, journal).await
}

/// `POST` handler for `xavier skills rollback`.
pub async fn skills_rollback_handler(
    State(_state): State<CliState>,
    Json(payload): Json<SkillsRollbackRequest>,
) -> Response {
    let journal = match Journal::from_settings() {
        Ok(j) => j,
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    rollback_skills(payload, journal).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_package(source: &Path, name: &str) {
        let dir = source.join(name);
        fs::create_dir_all(&dir).expect("mkdir package");
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\n---\n\nBody.\n"),
        )
        .expect("write SKILL.md");
    }

    fn manifest_toml(source: &Path, target: &Path) -> String {
        format!(
            "version = 1\n[sources]\ncanonical = \"{}\"\n\
             [[targets]]\nid = \"t1\"\ntools = [\"codex\"]\nroot = \"{}\"\nmode = \"symlink\"\n\
             [[placements]]\nsource = \"canonical\"\npath = \"demo\"\nname = \"demo\"\ntargets = [\"t1\"]\n",
            source.display(),
            target.display(),
        )
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .expect("read body");
        serde_json::from_slice(&bytes).expect("parse JSON body")
    }

    #[tokio::test]
    async fn plan_writes_nothing() {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&source).expect("mkdir source");
        fs::create_dir_all(&target).expect("mkdir target");
        write_package(&source, "demo");
        let manifest_path = tmp.path().join("manifest.toml");
        fs::write(&manifest_path, manifest_toml(&source, &target)).expect("write manifest");

        let resp = plan_skills(SkillsPlanRequest {
            manifest_path: manifest_path.to_string_lossy().into_owned(),
        })
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["plan"]["entries"][0]["action"], "create");
        assert!(
            fs::read_dir(&target).expect("read target").next().is_none(),
            "plan must not write to the target root"
        );
    }

    #[tokio::test]
    async fn apply_rejects_empty_plan_path_before_reading_manifest() {
        let tmp = TempDir::new().expect("tmp");
        let journal = Journal::new(tmp.path().join("state")).expect("journal");

        let resp = apply_skills(
            SkillsApplyRequest {
                manifest_path: tmp
                    .path()
                    .join("missing-manifest.toml")
                    .to_string_lossy()
                    .into_owned(),
                plan_path: String::new(),
                dry_run: false,
            },
            journal,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(body["error"]
            .as_str()
            .expect("error message")
            .contains("--plan"));
    }

    #[tokio::test]
    async fn apply_without_saved_plan_fails_before_mutation() {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&source).expect("mkdir source");
        fs::create_dir_all(&target).expect("mkdir target");
        write_package(&source, "demo");
        let manifest_path = tmp.path().join("manifest.toml");
        fs::write(&manifest_path, manifest_toml(&source, &target)).expect("write manifest");
        let journal = Journal::new(tmp.path().join("state")).expect("journal");

        let resp = apply_skills(
            SkillsApplyRequest {
                manifest_path: manifest_path.to_string_lossy().into_owned(),
                plan_path: tmp
                    .path()
                    .join("missing-plan.json")
                    .to_string_lossy()
                    .into_owned(),
                dry_run: false,
            },
            journal.clone(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            fs::read_dir(&target).expect("read target").next().is_none(),
            "no mutation without a saved plan"
        );
        assert!(
            journal
                .list_transactions()
                .expect("list transactions")
                .is_empty(),
            "no transaction recorded without a saved plan"
        );
    }

    #[tokio::test]
    async fn rollback_reports_conflict_and_preserves_edited_entry() {
        let tmp = TempDir::new().expect("tmp");
        let source = tmp.path().join("store");
        let target = tmp.path().join("tools/skills");
        fs::create_dir_all(&source).expect("mkdir source");
        fs::create_dir_all(&target).expect("mkdir target");
        write_package(&source, "demo");
        let manifest_path = tmp.path().join("manifest.toml");
        fs::write(&manifest_path, manifest_toml(&source, &target)).expect("write manifest");
        let journal = Journal::new(tmp.path().join("state")).expect("journal");

        let plan_resp = plan_skills(SkillsPlanRequest {
            manifest_path: manifest_path.to_string_lossy().into_owned(),
        })
        .await;
        let plan_body = body_json(plan_resp).await;
        let plan_path = tmp.path().join("plan.json");
        fs::write(
            &plan_path,
            serde_json::to_string(&plan_body["plan"]).expect("serialize plan"),
        )
        .expect("write plan");

        let apply_resp = apply_skills(
            SkillsApplyRequest {
                manifest_path: manifest_path.to_string_lossy().into_owned(),
                plan_path: plan_path.to_string_lossy().into_owned(),
                dry_run: false,
            },
            journal.clone(),
        )
        .await;
        assert_eq!(apply_resp.status(), StatusCode::OK);
        let apply_body = body_json(apply_resp).await;
        let tx_id = apply_body["tx_id"].as_str().expect("tx_id").to_string();

        let dest = target.join("demo");
        assert!(
            fs::symlink_metadata(&dest)
                .expect("dest metadata")
                .file_type()
                .is_symlink(),
            "apply must publish an owned symlink"
        );

        fs::remove_file(&dest).expect("remove owned link");
        fs::write(&dest, "edited by someone else").expect("write edited content");

        let rollback_resp = rollback_skills(
            SkillsRollbackRequest {
                manifest_path: manifest_path.to_string_lossy().into_owned(),
                tx_id,
                dry_run: false,
            },
            journal,
        )
        .await;
        assert_eq!(rollback_resp.status(), StatusCode::OK);
        let rollback_body = body_json(rollback_resp).await;
        assert_eq!(rollback_body["entries"][0]["outcome"]["kind"], "conflict");

        let content = fs::read_to_string(&dest).expect("read dest");
        assert_eq!(
            content, "edited by someone else",
            "an entry edited since apply must be preserved, not reverted"
        );
    }
}
