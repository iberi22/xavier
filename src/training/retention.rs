//! Retention of training job artifacts.
//!
//! Only terminal jobs (`succeeded`/`failed`/`cancelled`) are ever touched, and only their
//! heavy directories (`out/`, `artifacts/`, `notebook/`); `logs.txt` and the DB row stay.
//! A job whose artifact is referenced by a non-retired mini-expert is never pruned, and if
//! the registry cannot be read no succeeded job is pruned at all.
//!
//! Order: (1) age: unprotected terminal jobs older than `retention_days`;
//! (2) budget: while the heavy directories exceed `budget_bytes`, failed/cancelled jobs first,
//! then unprotected succeeded jobs, each oldest first.

use super::jobs::{JobRepository, JobStatus, TrainingJob};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DEFAULT_RETENTION_DAYS: u64 = 30;
pub const DEFAULT_ARTIFACTS_BUDGET_BYTES: u64 = 20 * 1024 * 1024 * 1024;
/// How often the periodic task runs.
pub const PRUNE_INTERVAL_SECS: u64 = 6 * 3600;

/// Directories of a job that hold bulky, regenerable output.
const HEAVY_DIRS: [&str; 3] = ["out", "artifacts", "notebook"];

/// Returns the gguf paths referenced by the mini-expert registry.
pub type ReferencedPaths = Arc<dyn Fn() -> Result<Vec<PathBuf>> + Send + Sync>;

#[derive(Clone)]
pub struct RetentionPolicy {
    /// 0 disables age-based pruning.
    pub retention_days: u64,
    /// 0 disables budget-based pruning.
    pub budget_bytes: u64,
    pub referenced: ReferencedPaths,
}

impl RetentionPolicy {
    /// `XAVIER_TRAIN_RETENTION_DAYS` (default 30, 0 = keep forever) and
    /// `XAVIER_TRAIN_ARTIFACTS_BUDGET_BYTES` (default 20 GiB, 0 = unlimited).
    /// References come from the mini-expert registry (non-retired versions).
    pub fn from_env() -> Self {
        let num = |k: &str, d: u64| -> u64 {
            std::env::var(k)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(d)
        };
        Self {
            retention_days: num("XAVIER_TRAIN_RETENTION_DAYS", DEFAULT_RETENTION_DAYS),
            budget_bytes: num(
                "XAVIER_TRAIN_ARTIFACTS_BUDGET_BYTES",
                DEFAULT_ARTIFACTS_BUDGET_BYTES,
            ),
            referenced: Arc::new(registry_references),
        }
    }

    /// Never prunes anything.
    pub fn disabled() -> Self {
        Self {
            retention_days: 0,
            budget_bytes: 0,
            referenced: Arc::new(|| Ok(Vec::new())),
        }
    }

    fn is_enabled(&self) -> bool {
        self.retention_days > 0 || self.budget_bytes > 0
    }
}

/// GGUF paths of every non-retired expert version in the registry.
fn registry_references() -> Result<Vec<PathBuf>> {
    use crate::agents::mini_experts::{ExpertStatus, ExpertStore};
    Ok(ExpertStore::open_default()?
        .list()?
        .into_iter()
        .filter(|r| r.status != ExpertStatus::Retired && !r.gguf_path.trim().is_empty())
        .map(|r| PathBuf::from(r.gguf_path))
        .collect())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub pruned_jobs: Vec<String>,
    pub freed_bytes: u64,
    /// Jobs skipped because a mini-expert references their artifact.
    pub protected_jobs: usize,
}

fn dir_size(p: &Path) -> u64 {
    let Ok(md) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if md.is_file() {
        return md.len();
    }
    if !md.is_dir() {
        return 0;
    }
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| dir_size(&e.path())).sum())
        .unwrap_or(0)
}

fn heavy_size(job_dir: &Path) -> u64 {
    HEAVY_DIRS.iter().map(|d| dir_size(&job_dir.join(d))).sum()
}

fn same_or_under(path: &Path, dir: &Path) -> bool {
    if path.starts_with(dir) {
        return true;
    }
    match (path.canonicalize(), dir.canonicalize()) {
        (Ok(p), Ok(d)) => p.starts_with(d),
        _ => false,
    }
}

/// True when a referenced path points inside the job's directory or at its artifact.
fn is_referenced(job: &TrainingJob, job_dir: &Path, refs: &[PathBuf]) -> bool {
    refs.iter().any(|r| {
        same_or_under(r, job_dir)
            || job
                .artifact_path
                .as_deref()
                .is_some_and(|a| Path::new(a) == r || same_or_under(r, Path::new(a)))
    })
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

struct Candidate {
    job: TrainingJob,
    dir: PathBuf,
    size: u64,
    at: DateTime<Utc>,
    protected: bool,
}

/// Applies the policy once. Pure with respect to time (`now` is injected).
pub fn prune(
    repo: &JobRepository,
    jobs_root: &Path,
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
) -> Result<PruneReport> {
    let mut report = PruneReport::default();
    if !policy.is_enabled() {
        return Ok(report);
    }
    let refs = match (policy.referenced)() {
        Ok(r) => Some(r),
        Err(e) => {
            tracing::warn!(error = %e, "training retention: registry unreadable, keeping succeeded jobs");
            None
        }
    };

    let mut total: u64 = 0;
    let mut cands: Vec<Candidate> = Vec::new();
    for job in repo.list(100_000)? {
        if !safe_id(&job.id) {
            continue;
        }
        let dir = jobs_root.join(&job.id);
        let size = heavy_size(&dir);
        total += size;
        if !job.status.is_terminal() || size == 0 {
            continue;
        }
        let protected = job.status == JobStatus::Succeeded
            && refs
                .as_ref()
                .map(|r| is_referenced(&job, &dir, r))
                .unwrap_or(true);
        let at = DateTime::parse_from_rfc3339(&job.updated_at)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or(now);
        if protected {
            report.protected_jobs += 1;
        }
        cands.push(Candidate {
            job,
            dir,
            size,
            at,
            protected,
        });
    }

    let remove = |c: &Candidate, total: &mut u64, report: &mut PruneReport| {
        for d in HEAVY_DIRS {
            let _ = std::fs::remove_dir_all(c.dir.join(d));
        }
        if let Err(e) = repo.clear_pruned_artifact(&c.job.id) {
            tracing::warn!(job = %c.job.id, error = %e, "training retention: could not clear artifact path");
        }
        *total = total.saturating_sub(c.size);
        report.freed_bytes += c.size;
        report.pruned_jobs.push(c.job.id.clone());
    };

    // Phase 1: age.
    let mut remaining: Vec<Candidate> = Vec::new();
    let cutoff = now - Duration::days(policy.retention_days.min(36_500) as i64);
    for c in cands {
        if policy.retention_days > 0 && !c.protected && c.at < cutoff {
            remove(&c, &mut total, &mut report);
        } else {
            remaining.push(c);
        }
    }

    // Phase 2: budget. Failed/cancelled before succeeded, oldest first.
    if policy.budget_bytes > 0 && total > policy.budget_bytes {
        remaining.retain(|c| !c.protected);
        remaining.sort_by_key(|c| (c.job.status == JobStatus::Succeeded, c.at));
        for c in &remaining {
            if total <= policy.budget_bytes {
                break;
            }
            remove(c, &mut total, &mut report);
        }
        if total > policy.budget_bytes {
            tracing::warn!(
                total,
                budget = policy.budget_bytes,
                "training retention: still over budget (remaining jobs are active or referenced)"
            );
        }
    }
    if !report.pruned_jobs.is_empty() {
        tracing::info!(
            jobs = report.pruned_jobs.len(),
            freed_bytes = report.freed_bytes,
            "training retention pruned job artifacts"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::training::jobs::{JobUpdate, NewJob};

    struct Env {
        _t: tempfile::TempDir,
        root: PathBuf,
        repo: JobRepository,
    }

    fn env() -> Env {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("jobs");
        std::fs::create_dir_all(&root).unwrap();
        Env {
            root,
            repo: JobRepository::open(&t.path().join("t.sqlite3")).unwrap(),
            _t: t,
        }
    }

    /// Creates a job in `status` with `bytes` of artifact under `artifacts/model.gguf`.
    fn job(e: &Env, status: JobStatus, bytes: usize) -> TrainingJob {
        let j = e
            .repo
            .create(NewJob {
                owner: "root".into(),
                domain: "d".into(),
                bundle_path: "/b".into(),
                bundle_hash: "h".into(),
                backend: "local".into(),
                base_model: "m".into(),
            })
            .unwrap();
        let dir = e.root.join(&j.id);
        std::fs::create_dir_all(dir.join("artifacts")).unwrap();
        std::fs::create_dir_all(dir.join("out")).unwrap();
        let gguf = dir.join("artifacts/model.gguf");
        std::fs::write(&gguf, vec![0u8; bytes]).unwrap();
        std::fs::write(dir.join("logs.txt"), b"log").unwrap();
        e.repo
            .update(
                &j.id,
                JobUpdate {
                    status: Some(status),
                    artifact_path: Some(gguf.to_string_lossy().into_owned()),
                    ..Default::default()
                },
            )
            .unwrap();
        e.repo.get(&j.id).unwrap().unwrap()
    }

    fn age(e: &Env, id: &str, days: i64) {
        let ts = (Utc::now() - Duration::days(days)).to_rfc3339();
        e.repo.set_updated_at_for_test(id, &ts);
    }

    fn policy(days: u64, budget: u64, refs: Vec<PathBuf>) -> RetentionPolicy {
        RetentionPolicy {
            retention_days: days,
            budget_bytes: budget,
            referenced: Arc::new(move || Ok(refs.clone())),
        }
    }

    fn payload_exists(e: &Env, id: &str) -> bool {
        e.root.join(id).join("artifacts/model.gguf").exists()
    }

    #[test]
    fn age_prunes_old_terminal_only_and_keeps_logs() {
        let e = env();
        let old_failed = job(&e, JobStatus::Failed, 10);
        let old_ok = job(&e, JobStatus::Succeeded, 10);
        let fresh_ok = job(&e, JobStatus::Succeeded, 10);
        let old_running = job(&e, JobStatus::Running, 10);
        for j in [&old_failed, &old_ok, &old_running] {
            age(&e, &j.id, 45);
        }
        let r = prune(&e.repo, &e.root, &policy(30, 0, vec![]), Utc::now()).unwrap();
        assert_eq!(r.pruned_jobs.len(), 2);
        assert!(!payload_exists(&e, &old_failed.id));
        assert!(!payload_exists(&e, &old_ok.id));
        assert!(payload_exists(&e, &fresh_ok.id));
        // active jobs are never touched, even when old
        assert!(payload_exists(&e, &old_running.id));
        // logs and row survive; artifact path is cleared
        assert!(e.root.join(&old_failed.id).join("logs.txt").exists());
        assert!(e
            .repo
            .get(&old_ok.id)
            .unwrap()
            .unwrap()
            .artifact_path
            .is_none());
    }

    #[test]
    fn referenced_artifact_is_never_deleted() {
        let e = env();
        let referenced = job(&e, JobStatus::Succeeded, 10);
        let other = job(&e, JobStatus::Succeeded, 10);
        age(&e, &referenced.id, 90);
        age(&e, &other.id, 90);
        let refs = vec![PathBuf::from(referenced.artifact_path.clone().unwrap())];
        // Both age and a tiny budget would remove it.
        let r = prune(&e.repo, &e.root, &policy(30, 1, refs), Utc::now()).unwrap();
        assert!(
            payload_exists(&e, &referenced.id),
            "referenced artifact deleted"
        );
        assert!(!payload_exists(&e, &other.id));
        assert_eq!(r.protected_jobs, 1);
        assert!(e
            .repo
            .get(&referenced.id)
            .unwrap()
            .unwrap()
            .artifact_path
            .is_some());
    }

    #[test]
    fn unreadable_registry_keeps_succeeded_but_prunes_failed() {
        let e = env();
        let ok = job(&e, JobStatus::Succeeded, 10);
        let failed = job(&e, JobStatus::Failed, 10);
        age(&e, &ok.id, 90);
        age(&e, &failed.id, 90);
        let p = RetentionPolicy {
            retention_days: 30,
            budget_bytes: 0,
            referenced: Arc::new(|| Err(anyhow::anyhow!("registry locked"))),
        };
        prune(&e.repo, &e.root, &p, Utc::now()).unwrap();
        assert!(payload_exists(&e, &ok.id));
        assert!(!payload_exists(&e, &failed.id));
    }

    #[test]
    fn budget_prunes_failed_first_then_oldest_succeeded_and_stops_at_budget() {
        let e = env();
        let old_ok = job(&e, JobStatus::Succeeded, 100);
        let new_ok = job(&e, JobStatus::Succeeded, 100);
        let newer_failed = job(&e, JobStatus::Cancelled, 100);
        let pinned = job(&e, JobStatus::Succeeded, 100);
        age(&e, &old_ok.id, 5);
        age(&e, &new_ok.id, 2);
        age(&e, &newer_failed.id, 1);
        age(&e, &pinned.id, 10);
        let refs = vec![PathBuf::from(pinned.artifact_path.clone().unwrap())];
        // 400 bytes total, budget 250: must drop two jobs: cancelled first, then oldest
        // unprotected succeeded. The pinned (oldest) one is skipped.
        let r = prune(&e.repo, &e.root, &policy(0, 250, refs), Utc::now()).unwrap();
        assert_eq!(
            r.pruned_jobs,
            vec![newer_failed.id.clone(), old_ok.id.clone()]
        );
        assert!(payload_exists(&e, &new_ok.id));
        assert!(payload_exists(&e, &pinned.id));
    }

    #[test]
    fn disabled_policy_is_a_noop() {
        let e = env();
        let j = job(&e, JobStatus::Failed, 10);
        age(&e, &j.id, 400);
        let r = prune(&e.repo, &e.root, &RetentionPolicy::disabled(), Utc::now()).unwrap();
        assert!(r.pruned_jobs.is_empty());
        assert!(payload_exists(&e, &j.id));
    }

    #[test]
    fn env_defaults_are_30_days_and_20_gib() {
        // Only checks the constants/parse path without mutating process env.
        assert_eq!(DEFAULT_RETENTION_DAYS, 30);
        assert_eq!(DEFAULT_ARTIFACTS_BUDGET_BYTES, 20 * 1024 * 1024 * 1024);
    }
}
