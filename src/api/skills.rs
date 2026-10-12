//! Skills API endpoints
//!
//! Provides HTTP endpoints for skill dispatch and memory health monitoring.
//! These endpoints allow external agents (IDEs, CLI tools, bots) to leverage
//! Xavier's skill orchestration and cognitive maintenance capabilities.

use axum::{extract::Extension, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::skill_controller::state::{Journal, JournalAction};

use crate::security::clearance::{default_clearance, split_by_clearance, ClearanceLevel};
use crate::workspace::WorkspaceContext;

/// Production skill routes, shared with transport regression tests.
pub fn skill_api_routes<S: Clone + Send + Sync + 'static>() -> axum::Router<S> {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/api/skill/dispatch", post(dispatch_skill))
        .route("/api/skill/list", get(list_skills))
        .route("/skills", get(list_skills))
        .route("/api/skill/usage", get(skill_usage))
        .route("/api/skill/audit", get(audit_skills))
}

use crate::context::skill_controller::audit::{
    audit_packages, AuditReport, AuditedPackage, Severity,
};
use crate::context::{SkillFsPolicy, SkillManifest, SkillResolvedRoots};

// ---------------------------------------------------------------------------
// POST /api/skill/dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DispatchRequest {
    /// The task description
    pub task: String,
    /// Optional model hint for budget estimation (e.g. "claude-opus-4")
    #[serde(default)]
    pub model_hint: Option<String>,
    /// Maximum tokens the agent can afford
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    /// Optional project filter for memory retrieval
    #[serde(default)]
    pub project: Option<String>,
}

fn default_max_tokens() -> usize {
    4000
}

#[derive(Debug, Serialize)]
pub struct DispatchResponse {
    pub skill_name: String,
    pub skill_description: String,
    pub confidence: f32,
    pub context_pack: ContextPackResponse,
    pub estimated_savings_pct: f32,
}

#[derive(Debug, Serialize)]
pub struct ContextPackResponse {
    pub system_instructions: String,
    pub relevant_memories: Vec<MemoryRefResponse>,
    pub prior_decisions: Vec<String>,
    pub total_tokens: usize,
}

#[derive(Debug, Serialize)]
pub struct MemoryRefResponse {
    pub id: String,
    pub path: String,
    pub summary: String,
    pub keywords: Vec<String>,
}

/// Dispatch skill.
pub async fn dispatch_skill(
    Extension(workspace): Extension<WorkspaceContext>,
    requester: Option<Extension<ClearanceLevel>>,
    Json(request): Json<DispatchRequest>,
) -> impl IntoResponse {
    use crate::context::skill_dispatcher::SkillDispatchRequest;
    use crate::context::skill_registry::SkillRegistry;
    use crate::context::SkillDispatcher;

    let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
    let registry = SkillRegistry::build_skill_registry(&workspace_root).await;
    let memory = workspace.workspace.memory.clone();
    let dispatcher = SkillDispatcher::new(registry, Some(memory))
        .with_clearance(
            requester
                .map(|Extension(level)| level)
                .unwrap_or_else(default_clearance),
        )
        .with_telemetry_db(skill_usage_db_path());

    let dispatch_request = SkillDispatchRequest {
        task: request.task,
        model_hint: request.model_hint,
        max_tokens: Some(request.max_tokens),
        project: request.project,
    };

    match dispatcher.dispatch(&dispatch_request).await {
        Ok(result) => {
            let response = DispatchResponse {
                skill_name: result.skill_name,
                skill_description: result.skill_description,
                confidence: result.confidence,
                context_pack: ContextPackResponse {
                    system_instructions: result.context_pack.system_instructions,
                    relevant_memories: result
                        .context_pack
                        .relevant_memories
                        .into_iter()
                        .map(|m| MemoryRefResponse {
                            id: m.id,
                            path: m.path,
                            summary: m.summary,
                            keywords: m.keywords,
                        })
                        .collect(),
                    prior_decisions: result.context_pack.prior_decisions,
                    total_tokens: result.context_pack.total_tokens,
                },
                estimated_savings_pct: result.estimated_savings_pct,
            };
            Json(serde_json::json!({ "ok": true, "data": response }))
        }
        Err(e) => Json(serde_json::json!({
            "ok": false,
            "error": format!("{}", e)
        })),
    }
}

// ---------------------------------------------------------------------------
// GET /api/memory/health
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct MemoryHealthResponse {
    pub total_documents: usize,
    pub total_size_bytes: u64,
    pub by_priority: std::collections::HashMap<String, usize>,
    pub low_quality_count: usize,
    pub ephemeral_count: usize,
    pub decayed_count: usize,
}

/// Memory health.
pub async fn memory_health(Extension(workspace): Extension<WorkspaceContext>) -> impl IntoResponse {
    let memory = &workspace.workspace.memory;
    let docs = memory.all_documents().await;
    let total_documents = docs.len();
    let total_size_bytes: u64 = docs.iter().map(|d| d.estimated_bytes()).sum();

    use crate::memory::manager::MemoryPriority;
    let mut by_priority = std::collections::HashMap::new();
    let mut low_quality_count = 0;
    let mut ephemeral_count = 0;

    for doc in &docs {
        let priority = MemoryPriority::from_metadata(&doc.metadata);
        *by_priority
            .entry(priority.as_str().to_string())
            .or_insert(0) += 1;

        if priority == MemoryPriority::Ephemeral {
            ephemeral_count += 1;
        }
        if doc.content.trim().len() < 10 {
            low_quality_count += 1;
        }
    }

    let response = MemoryHealthResponse {
        total_documents,
        total_size_bytes,
        by_priority,
        low_quality_count,
        ephemeral_count,
        decayed_count: 0, // Would need MemoryManager access for accurate count
    };

    Json(serde_json::json!({ "ok": true, "data": response }))
}

// ---------------------------------------------------------------------------
// GET /api/skill/list
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct SkillListEntry {
    pub name: String,
    pub description: String,
    pub domains: Vec<String>,
    pub token_cost: usize,
}

/// List skills.
pub async fn list_skills(
    Extension(workspace): Extension<WorkspaceContext>,
    requester: Option<Extension<ClearanceLevel>>,
) -> impl IntoResponse {
    use crate::context::skill_registry::SkillRegistry;

    let workspace_root = std::path::PathBuf::from(&workspace.workspace_id);
    let registry = SkillRegistry::build_skill_registry(&workspace_root).await;

    let (visible, _) = split_by_clearance(
        requester
            .map(|Extension(level)| level)
            .unwrap_or_else(default_clearance),
        registry.list(),
        |name| registry.clearance_of(name),
    );
    let skills: Vec<SkillListEntry> = visible
        .into_iter()
        .filter_map(|name| {
            registry.get(name).map(|s| SkillListEntry {
                name: s.name.clone(),
                description: s.description.clone(),
                domains: s.domains.clone(),
                token_cost: s.token_cost,
            })
        })
        .collect();

    Json(serde_json::json!({
        "ok": true,
        "count": skills.len(),
        "skills": skills
    }))
}

/// Dedicated skill telemetry database, separate from the memory store.
pub fn skill_usage_db_path() -> PathBuf {
    Journal::default_dir().join("usage.sqlite3")
}

/// Read only aggregate lifecycle counts; dispatch alone is not proof of use.
pub fn usage_summary_blocking(db: &Path) -> Result<serde_json::Value, String> {
    use crate::context::skill_controller::telemetry::RetentionPolicy;
    let retention_days = match RetentionPolicy::default_policy() {
        RetentionPolicy::Expire { max_age_days } => Some(max_age_days),
        RetentionPolicy::KeepForever => None,
    };
    let mut skills = Vec::new();
    let conn =
        match rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        {
            Ok(conn) => Some(conn),
            Err(_) if !db.try_exists().map_err(|e| e.to_string())? => None,
            Err(error) => return Err(error.to_string()),
        };
    if let Some(conn) = conn {
        let mut stmt = conn
            .prepare(
                "SELECT skill_name, SUM(stage = 'selected'), SUM(stage = 'delivered'),
                    SUM(stage = 'invoked'), SUM(stage = 'completed')
             FROM skill_usage_events GROUP BY skill_name ORDER BY skill_name",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                let invoked: u64 = row.get(3)?;
                let completed: u64 = row.get(4)?;
                Ok(serde_json::json!({
                    "skill_name": row.get::<_, String>(0)?,
                    "selected": row.get::<_, u64>(1)?,
                    "delivered": row.get::<_, u64>(2)?,
                    "invoked": invoked,
                    "completed": completed,
                    "invocation_coverage": if invoked > 0 || completed > 0 {
                        "observed"
                    } else {
                        "unknown"
                    },
                }))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            skills.push(row.map_err(|e| e.to_string())?);
        }
    }
    Ok(serde_json::json!({
        "retention_days": retention_days,
        "skills": skills,
        "note": "dispatch is not proof of use; unknown means no harness acknowledgment",
    }))
}

pub async fn skill_usage() -> axum::response::Response {
    match tokio::task::spawn_blocking(|| usage_summary_blocking(&skill_usage_db_path())).await {
        Ok(Ok(summary)) => (axum::http::StatusCode::OK, Json(summary)).into_response(),
        Ok(Err(error)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": error})),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "skill usage task failed"})),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/skill/audit
// ---------------------------------------------------------------------------

pub fn audit_report_json(report: &AuditReport) -> serde_json::Value {
    serde_json::json!({
        "packages_audited": report.packages_audited,
        "bytes_scanned": report.bytes_scanned,
        "truncated": report.truncated,
        "findings": report.findings.iter().map(|f| {
            serde_json::json!({
                "rule": f.rule.id,
                "severity": match f.severity {
                    Severity::Info => "info",
                    Severity::Warning => "warning",
                    Severity::Error => "error",
                },
                "package": f.package,
                "location": f.location,
                "detail": f.detail,
            })
        }).collect::<Vec<_>>(),
        "proposals": report.proposals.iter().map(|p| {
            serde_json::json!({
                "rule": p.rule.id,
                "package": p.package,
                "path": p.path,
                "diff": p.diff,
            })
        }).collect::<Vec<_>>(),
    })
}

fn recorded_hashes(journal_dir: &Path) -> HashMap<PathBuf, String> {
    if !journal_dir.exists() {
        return HashMap::new();
    }
    let transactions = match Journal::new(journal_dir.to_path_buf())
        .and_then(|journal| journal.committed_transactions())
    {
        Ok(transactions) => transactions,
        Err(error) => {
            tracing::debug!(%error, "Could not read skill projection journal");
            return HashMap::new();
        }
    };
    let mut hashes = HashMap::new();
    for transaction in transactions {
        for entry in transaction.entries {
            if let (JournalAction::LinkSymlink { source }, Some(hash)) =
                (entry.action, entry.owned_hash)
            {
                let Ok(source) = source.canonicalize() else {
                    continue;
                };
                let recorded = hashes
                    .entry(source)
                    .or_insert((transaction.updated_at, hash.clone()));
                if transaction.updated_at > recorded.0 {
                    *recorded = (transaction.updated_at, hash);
                }
            }
        }
    }
    hashes
        .into_iter()
        .map(|(source, (_, hash))| (source, hash))
        .collect()
}

pub fn audit_manifest_blocking(
    manifest_path: &std::path::Path,
) -> Result<serde_json::Value, String> {
    audit_manifest_with_journal(manifest_path, &Journal::default_dir())
}

fn audit_manifest_with_journal(
    manifest_path: &Path,
    journal_dir: &Path,
) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let manifest = SkillManifest::from_toml(&content).map_err(|e| e.to_string())?;

    let sources: Vec<std::path::PathBuf> = manifest
        .sources
        .values()
        .map(std::path::PathBuf::from)
        .collect();
    let targets: Vec<std::path::PathBuf> = manifest
        .targets
        .iter()
        .map(|t| std::path::PathBuf::from(&t.root))
        .collect();
    let policy = SkillFsPolicy::new(&sources, &targets).map_err(|e| e.to_string())?;
    let roots = SkillResolvedRoots {
        sources: manifest
            .sources
            .iter()
            .map(|(id, dir)| (id.clone(), std::path::PathBuf::from(dir)))
            .collect(),
        targets: manifest
            .targets
            .iter()
            .map(|t| (t.id.clone(), std::path::PathBuf::from(&t.root)))
            .collect(),
    };

    let hashes = recorded_hashes(journal_dir);
    let packages: Vec<AuditedPackage> = manifest
        .placements
        .iter()
        .map(|p| AuditedPackage {
            placement: p.clone(),
            recorded_hash: roots
                .sources
                .get(&p.source)
                .and_then(|root| root.join(&p.path).canonicalize().ok())
                .and_then(|path| hashes.get(&path).cloned()),
        })
        .collect();

    let report = audit_packages(&policy, &roots, &packages).map_err(|e| e.to_string())?;

    Ok(audit_report_json(&report))
}

pub async fn audit_skills() -> axum::response::Response {
    let path_str = std::env::var("XAVIER_SKILL_MANIFEST").unwrap_or_default();
    let path_str = path_str.trim();
    if path_str.is_empty() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no skill manifest configured"})),
        )
            .into_response();
    }
    let path = std::path::PathBuf::from(path_str);

    match tokio::task::spawn_blocking(move || audit_manifest_blocking(&path)).await {
        Ok(Ok(json)) => (axum::http::StatusCode::OK, Json(json)).into_response(),
        Ok(Err(msg)) => (
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({"error": msg})),
        )
            .into_response(),
        Err(_) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "audit task failed"})),
        )
            .into_response(),
    }
}

#[cfg(test)]
pub(crate) fn seed_usage_fixture(db: &Path) {
    use crate::context::skill_controller::telemetry::{SkillStage, TelemetryWriter, UsageEvent};
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let conn = rusqlite::Connection::open(db).unwrap();
    let writer = TelemetryWriter::new(&conn);
    writer.ensure_schema().unwrap();
    for (id, skill, stage) in [
        ("evt-z", "z-selected", SkillStage::Selected),
        ("evt-a", "a-invoked", SkillStage::Selected),
        ("evt-d", "a-invoked", SkillStage::Delivered),
    ] {
        writer
            .record(
                &UsageEvent::new(
                    id,
                    "fixture-workspace",
                    "private-task-id",
                    "fixture-harness",
                    skill,
                    stage,
                ),
                false,
            )
            .unwrap();
    }
    drop(conn);
    for (id, stage) in [
        ("evt-i", SkillStage::Invoked),
        ("evt-c", SkillStage::Completed),
    ] {
        crate::context::skill_dispatcher::record_ack(
            db,
            &UsageEvent::new(
                id,
                "fixture-workspace",
                "private-task-id",
                "fixture-harness",
                "a-invoked",
                stage,
            ),
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::RuntimeConfig;
    use crate::memory::store::MemoryBackend;
    use crate::settings::types::DedupSettings;
    use crate::workspace::{
        EmbeddingProviderMode, PlanTier, SyncPolicy, WorkspaceConfig, WorkspaceState,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn skill_usage_counts_coverage_and_redaction() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("usage.sqlite3");
        seed_usage_fixture(&db);
        let before = std::fs::read(&db).unwrap();
        let summary = usage_summary_blocking(&db).unwrap();
        assert_eq!(summary["retention_days"], 30);
        assert_eq!(
            summary["skills"],
            serde_json::json!([
                {"skill_name": "a-invoked", "selected": 1, "delivered": 1,
                 "invoked": 1, "completed": 1, "invocation_coverage": "observed"},
                {"skill_name": "z-selected", "selected": 1, "delivered": 0,
                 "invoked": 0, "completed": 0, "invocation_coverage": "unknown"},
            ])
        );
        let output = summary.to_string();
        for private in [
            "private-task-id",
            "fixture-workspace",
            "fixture-harness",
            "evt-",
            "task text must not leak",
            temp.path().to_str().unwrap(),
        ] {
            assert!(!output.contains(private), "leaked fixture field");
        }
        assert_eq!(std::fs::read(&db).unwrap(), before);
    }

    #[test]
    fn skill_usage_invoked_ack_changes_unknown_to_observed() {
        use crate::context::skill_controller::telemetry::{
            SkillStage, TelemetryWriter, UsageEvent,
        };
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("usage.sqlite3");
        let conn = rusqlite::Connection::open(&db).unwrap();
        let writer = TelemetryWriter::new(&conn);
        writer.ensure_schema().unwrap();
        writer
            .record(
                &UsageEvent::new(
                    "selected",
                    "ws",
                    "private-task",
                    "harness",
                    "demo",
                    SkillStage::Selected,
                ),
                false,
            )
            .unwrap();
        drop(conn);
        let selected = usage_summary_blocking(&db).unwrap();
        assert_eq!(selected["skills"][0]["invocation_coverage"], "unknown");
        crate::context::skill_dispatcher::record_ack(
            &db,
            &UsageEvent::new(
                "ack",
                "ws",
                "private-task",
                "harness",
                "demo",
                SkillStage::Invoked,
            ),
        )
        .unwrap();
        let invoked = usage_summary_blocking(&db).unwrap();
        assert_eq!(invoked["skills"][0]["invoked"], 1);
        assert_eq!(invoked["skills"][0]["completed"], 0);
        assert_eq!(invoked["skills"][0]["invocation_coverage"], "observed");
    }

    #[test]
    fn skill_usage_missing_db_stays_absent() {
        let temp = tempfile::tempdir().unwrap();
        let db = temp.path().join("missing/usage.sqlite3");
        let summary = usage_summary_blocking(&db).unwrap();
        assert_eq!(summary["skills"], serde_json::json!([]));
        assert_eq!(summary["retention_days"], 30);
        assert!(!db.exists());
        assert!(!db.parent().unwrap().exists());
        let db = temp.path().join("usage.sqlite3");
        assert_eq!(usage_summary_blocking(&db).unwrap(), summary);
        assert!(!db.exists());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn skill_usage_invalid_db_returns_500() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp = tempfile::tempdir().unwrap();
        let _env = EnvGuard::isolate(temp.path());
        let db = skill_usage_db_path();
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::write(&db, "invalid SQLite fixture").unwrap();
        assert!(usage_summary_blocking(&db).is_err());
        assert_eq!(
            skill_usage().await.status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    struct AuditFixture {
        temp: tempfile::TempDir,
        manifest: PathBuf,
        package: PathBuf,
        journal_dir: PathBuf,
    }

    impl AuditFixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let sources = temp.path().join("sources");
            let targets = temp.path().join("targets");
            let package = sources.join("demo");
            std::fs::create_dir_all(&package).unwrap();
            std::fs::create_dir_all(&targets).unwrap();
            std::fs::write(
                package.join("SKILL.md"),
                "---\nname: demo\ndescription: Fixture skill\n---\nOriginal instructions\n",
            )
            .unwrap();
            let manifest = temp.path().join("manifest.toml");
            std::fs::write(
                &manifest,
                format!(
                    r#"version = 1
[sources]
fixture = "{}"
[[targets]]
id = "fixture"
root = "{}"
tools = ["codex"]
mode = "symlink"
[[placements]]
source = "fixture"
targets = ["fixture"]
path = "demo"
name = "demo"
"#,
                    sources.display(),
                    targets.display()
                ),
            )
            .unwrap();
            let journal_dir = temp.path().join("journal");
            Self {
                temp,
                manifest,
                package,
                journal_dir,
            }
        }

        fn project(&self) {
            use crate::context::skill_controller::backup::BackupKind;
            use crate::context::skill_controller::plan::hash_package;
            use crate::context::skill_controller::policy::RootKind;
            use crate::context::skill_controller::state::JournalEntry;

            let manifest =
                SkillManifest::from_toml(&std::fs::read_to_string(&self.manifest).unwrap())
                    .unwrap();
            let policy = SkillFsPolicy::new(
                &[self.temp.path().join("sources")],
                &[self.temp.path().join("targets")],
            )
            .unwrap();
            let dir = policy.authorize(RootKind::Source, &self.package).unwrap();
            let hash = hash_package(&dir, &manifest.placements[0]).unwrap().hash;
            let journal = Journal::new(self.journal_dir.clone()).unwrap();
            journal
                .begin_transaction("projection", vec![], false)
                .unwrap();
            journal
                .record_entry(
                    "projection",
                    JournalEntry {
                        target_path: self.temp.path().join("targets/demo"),
                        action: JournalAction::LinkSymlink {
                            source: self.package.clone(),
                        },
                        prior_state: BackupKind::Absent,
                        owned_hash: Some(hash),
                    },
                    false,
                )
                .unwrap();
            journal.commit_transaction("projection", false).unwrap();
        }

        fn audit(&self) -> serde_json::Value {
            audit_manifest_with_journal(&self.manifest, &self.journal_dir).unwrap()
        }

        fn edit(&self) {
            std::fs::write(
                self.package.join("SKILL.md"),
                "---\nname: demo\ndescription: Fixture skill\n---\nChanged instructions\n",
            )
            .unwrap();
        }
    }

    fn has_hash_drift(report: &serde_json::Value) -> bool {
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["rule"] == "skill.projection.hash_drift")
    }

    fn snapshot_files(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut files = std::collections::BTreeMap::new();
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(snapshot_files(&path));
            } else {
                files.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
        files
    }

    #[test]
    fn test_audit_projected_package_without_hash_drift() {
        let fixture = AuditFixture::new();
        fixture.project();
        assert!(!has_hash_drift(&fixture.audit()));
    }

    #[test]
    fn test_audit_edited_projected_package_reports_hash_drift() {
        let fixture = AuditFixture::new();
        fixture.project();
        fixture.edit();
        assert!(has_hash_drift(&fixture.audit()));
    }

    #[test]
    fn test_audit_never_projected_package_has_no_hash_drift() {
        let fixture = AuditFixture::new();
        Journal::new(fixture.journal_dir.clone()).unwrap();
        fixture.edit();
        assert!(!has_hash_drift(&fixture.audit()));
    }

    #[test]
    fn test_audit_missing_journal_does_not_create_directory() {
        let fixture = AuditFixture::new();
        assert!(!fixture.journal_dir.exists());
        assert!(!has_hash_drift(&fixture.audit()));
        assert!(!fixture.journal_dir.exists());
    }

    #[test]
    fn test_audit_preserves_journal_and_fixture_bytes() {
        let fixture = AuditFixture::new();
        fixture.project();
        fixture.edit();
        let before = snapshot_files(fixture.temp.path());
        assert!(has_hash_drift(&fixture.audit()));
        assert_eq!(before, snapshot_files(fixture.temp.path()));
    }

    #[test]
    fn test_recorded_hashes_uses_latest_updated_committed_symlink() {
        use crate::context::skill_controller::backup::BackupKind;
        use crate::context::skill_controller::state::JournalEntry;

        let fixture = AuditFixture::new();
        let journal = Journal::new(fixture.journal_dir.clone()).unwrap();
        for (id, created_at, updated_at, hash, committed) in [
            ("latest", 1, 20, "latest-hash", true),
            ("older", 2, 10, "older-hash", true),
            ("pending", 3, 30, "pending-hash", false),
        ] {
            let entry = JournalEntry {
                target_path: fixture.temp.path().join("targets/demo"),
                action: JournalAction::LinkSymlink {
                    source: fixture.package.join("../demo"),
                },
                prior_state: BackupKind::Absent,
                owned_hash: Some(hash.to_string()),
            };
            journal.begin_transaction(id, vec![entry], false).unwrap();
            let mut tx = if committed {
                journal.commit_transaction(id, false).unwrap()
            } else {
                journal.get_transaction(id).unwrap().unwrap()
            };
            tx.created_at = created_at;
            tx.updated_at = updated_at;
            std::fs::write(
                journal.transactions_dir().join(format!("{id}.json")),
                serde_json::to_vec(&tx).unwrap(),
            )
            .unwrap();
        }
        let ignored = vec![
            JournalEntry {
                target_path: fixture.temp.path().join("targets/demo"),
                action: JournalAction::WriteCopy {
                    source: fixture.package.clone(),
                    hash: "file-hash".into(),
                },
                prior_state: BackupKind::Absent,
                owned_hash: Some("copy-hash".into()),
            },
            JournalEntry {
                target_path: fixture.temp.path().join("targets/demo"),
                action: JournalAction::LinkSymlink {
                    source: fixture.package.clone(),
                },
                prior_state: BackupKind::Absent,
                owned_hash: None,
            },
        ];
        journal
            .begin_transaction("ignored", ignored, false)
            .unwrap();
        journal.commit_transaction("ignored", false).unwrap();
        assert_eq!(
            recorded_hashes(&fixture.journal_dir),
            HashMap::from([(
                fixture.package.canonicalize().unwrap(),
                "latest-hash".to_string()
            ),])
        );
    }

    #[tokio::test]
    async fn test_audit_hash_drift_blocking_and_endpoint_match() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let fixture = tokio::task::spawn_blocking(|| {
            let mut fixture = AuditFixture::new();
            fixture.journal_dir = fixture.temp.path().join("data/skill_controller");
            fixture.project();
            fixture.edit();
            fixture
        })
        .await
        .unwrap();
        let _env_guard = EnvGuard::isolate(fixture.temp.path());
        std::env::set_var("XAVIER_SKILL_MANIFEST", &fixture.manifest);
        let manifest = fixture.manifest.clone();
        let expected =
            tokio::task::spawn_blocking(move || audit_manifest_blocking(&manifest).unwrap())
                .await
                .unwrap();
        assert!(has_hash_drift(&expected));
        let response = audit_skills().await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_audit_journal_error_keeps_hash_unknown() {
        let fixture = AuditFixture::new();
        std::fs::write(&fixture.journal_dir, "not a directory").unwrap();
        assert!(!has_hash_drift(&fixture.audit()));
        assert_eq!(
            std::fs::read(&fixture.journal_dir).unwrap(),
            b"not a directory"
        );
    }

    /// Saves the previous `HOME` and `XAVIER_SKILL_STORE` values and restores both
    /// on drop, so a failing assertion unwinds without leaking either override into
    /// sibling tests of this binary.
    struct EnvGuard {
        home: Option<String>,
        skill_store: Option<String>,
        skill_manifest: Option<std::ffi::OsString>,
        data_dir: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        /// Point `HOME` at the test tempdir and `XAVIER_SKILL_STORE` at an empty
        /// store, so the registry never scans the developer's real
        /// `~/.hermes/skills` nor reads their `~/.hermes/config.yaml`.
        fn isolate(temp_dir: &std::path::Path) -> Self {
            let guard = Self {
                home: std::env::var("HOME").ok(),
                skill_store: std::env::var("XAVIER_SKILL_STORE").ok(),
                skill_manifest: std::env::var_os("XAVIER_SKILL_MANIFEST"),
                data_dir: std::env::var_os("XAVIER_DATA_DIR"),
            };
            let empty_store = temp_dir.join("empty_store");
            if !empty_store.exists() {
                std::fs::create_dir_all(&empty_store)
                    .expect("empty skill store should be creatable");
            }
            std::env::set_var("XAVIER_DATA_DIR", temp_dir.join("data"));
            std::env::set_var("HOME", temp_dir);
            std::env::set_var("XAVIER_SKILL_STORE", &empty_store);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.data_dir.take() {
                Some(value) => std::env::set_var("XAVIER_DATA_DIR", value),
                None => std::env::remove_var("XAVIER_DATA_DIR"),
            }
            match self.skill_manifest.take() {
                Some(value) => std::env::set_var("XAVIER_SKILL_MANIFEST", value),
                None => std::env::remove_var("XAVIER_SKILL_MANIFEST"),
            }
            for (key, previous) in [
                ("HOME", self.home.take()),
                ("XAVIER_SKILL_STORE", self.skill_store.take()),
            ] {
                match previous {
                    Some(val) => std::env::set_var(key, val),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    /// Build a fully explicit workspace config with a literal synthetic token:
    /// tests must never read `XAVIER_TOKEN` from the environment, which CI does not set.
    fn test_workspace_config() -> WorkspaceConfig {
        WorkspaceConfig {
            id: "test-skills-api".to_string(),
            token: "test-token".to_string(),
            plan: PlanTier::Personal,
            memory_backend: MemoryBackend::Memory,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(1000),
            request_unit_limit: Some(1000),
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::LocalOnly,
            protocol: Default::default(),
            dedup: DedupSettings::default(),
        }
    }

    async fn setup_test_workspace(temp_dir: &std::path::Path) -> WorkspaceContext {
        let skills_dir = temp_dir.join("skills").join("test-skill");
        tokio::fs::create_dir_all(&skills_dir).await.unwrap();
        let skill_file = skills_dir.join("SKILL.md");
        let content = r#"---
name: test-skill
description: A helpful test skill for memory testing
---
# Test Skill Instructions
Follow these steps to analyze memory and debug issues.
"#;
        tokio::fs::write(&skill_file, content).await.unwrap();

        let config = test_workspace_config();
        let runtime = RuntimeConfig::from_env();
        let state = Arc::new(
            WorkspaceState::new(config, runtime, temp_dir.to_path_buf())
                .await
                .unwrap(),
        );

        WorkspaceContext {
            workspace_id: temp_dir.to_string_lossy().to_string(),
            workspace: state,
        }
    }

    #[tokio::test]
    async fn test_list_skills_retains_json_fields() {
        crate::isolate_test_process!();
        // Same lock as the registry tests: both mutate XAVIER_SKILL_STORE in one test binary.
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let workspace_ctx = setup_test_workspace(temp_dir.path()).await;
        let app = skill_api_routes::<()>()
            .layer(Extension(ClearanceLevel::Confidential))
            .layer(Extension(workspace_ctx));

        let req = Request::builder()
            .uri("/api/skill/list")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["ok"], true);
        assert_eq!(json["count"], 1);
        let skills = json["skills"].as_array().unwrap();
        assert_eq!(skills.len(), 1);
        let skill = &skills[0];
        assert_eq!(skill["name"], "test-skill");
        assert_eq!(
            skill["description"],
            "A helpful test skill for memory testing"
        );
        assert!(skill["domains"].is_array());
        assert!(skill["token_cost"].is_number());
    }

    #[tokio::test]
    async fn test_dispatch_skill_matched_retains_json_fields() {
        crate::isolate_test_process!();
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let workspace_ctx = setup_test_workspace(temp_dir.path()).await;
        let app = skill_api_routes::<()>()
            .layer(Extension(ClearanceLevel::Confidential))
            .layer(Extension(workspace_ctx));

        let payload = serde_json::json!({
            "task": "A helpful test skill for memory testing",
            "max_tokens": 4000
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/skill/dispatch")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["ok"], true);
        let data = &json["data"];
        assert_eq!(data["skill_name"], "test-skill");
        assert_eq!(
            data["skill_description"],
            "A helpful test skill for memory testing"
        );
        assert!(data["confidence"].as_f64().unwrap() >= 0.40);
        assert!(data["estimated_savings_pct"].is_number());

        let context_pack = &data["context_pack"];
        assert!(context_pack["system_instructions"].is_string());
        assert!(context_pack["relevant_memories"].is_array());
        assert!(context_pack["prior_decisions"].is_array());
        assert!(context_pack["total_tokens"].is_number());
    }

    #[tokio::test]
    async fn test_dispatch_skill_unmatched_retains_json_fields() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let workspace_ctx = setup_test_workspace(temp_dir.path()).await;
        let app = Router::new()
            .route("/api/skill/dispatch", post(dispatch_skill))
            .layer(Extension(workspace_ctx));

        let payload = serde_json::json!({
            "task": "completely unrelated cooking recipe for pasta",
            "max_tokens": 4000
        });
        let req = Request::builder()
            .method("POST")
            .uri("/api/skill/dispatch")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(json["ok"], true);
        let data = &json["data"];
        assert_eq!(data["skill_name"], "_none");
        assert_eq!(data["skill_description"], "No matching skill found");
        assert_eq!(data["confidence"], 0.0);
        assert_eq!(data["estimated_savings_pct"], 0.0);

        let context_pack = &data["context_pack"];
        assert_eq!(context_pack["system_instructions"], "");
        assert!(context_pack["relevant_memories"].is_array());
        assert!(context_pack["prior_decisions"].is_array());
        assert_eq!(context_pack["total_tokens"], 0);
    }

    #[tokio::test]
    async fn test_audit_manifest_blocking_and_endpoint() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());

        let sources_dir = temp_dir.path().join("sources");
        let targets_dir = temp_dir.path().join("targets");
        let package_dir = sources_dir.join("demo");
        std::fs::create_dir_all(&package_dir).unwrap();
        std::fs::create_dir_all(&targets_dir).unwrap();

        let skill_md_path = package_dir.join("SKILL.md");
        let content = "---\nname: demo\n---\nbody";
        let before_content = content.as_bytes().to_vec();
        std::fs::write(&skill_md_path, content).unwrap();

        let manifest_path = temp_dir.path().join("manifest.toml");
        let manifest_content = format!(
            r#"version = 1

[sources]
test_source = "{}"

[[targets]]
id = "test_target"
root = "{}"
tools = ["codex"]
mode = "symlink"

[[placements]]
source = "test_source"
targets = ["test_target"]
path = "demo"
name = "demo"
"#,
            sources_dir.display(),
            targets_dir.display()
        );
        std::fs::write(&manifest_path, &manifest_content).unwrap();
        let before_files = vec![
            (skill_md_path.clone(), before_content),
            (manifest_path.clone(), manifest_content.into_bytes()),
        ];

        let result = audit_manifest_blocking(&manifest_path).unwrap();
        let findings = result.get("findings").unwrap().as_array().unwrap();
        assert!(
            !findings.is_empty(),
            "Expected findings due to missing description"
        );

        std::env::set_var(
            "XAVIER_SKILL_MANIFEST",
            format!("  {}  ", manifest_path.display()),
        );
        let app = Router::new().route("/api/skill/audit", get(audit_skills));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/skill/audit")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let (parts, body) = resp.into_parts();
        assert_eq!(parts.status, StatusCode::OK);

        let body_bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();

        let endpoint_findings = json.get("findings").unwrap().as_array().unwrap();
        assert_eq!(findings, endpoint_findings);

        for (path, bytes) in before_files {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read_dir(&targets_dir).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn test_audit_skills_unset_manifest_returns_404() {
        let _lock = crate::context::skill_registry::env_lock().lock().await;
        let temp_dir = tempfile::tempdir().unwrap();
        let _env_guard = EnvGuard::isolate(temp_dir.path());
        std::env::remove_var("XAVIER_SKILL_MANIFEST");
        let resp_not_found = audit_skills().await;
        let (parts_not_found, _) = resp_not_found.into_parts();
        assert_eq!(parts_not_found.status, StatusCode::NOT_FOUND);
    }
}
