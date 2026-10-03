//! `ComputeBackend`: where a training job runs. Only two backends exist by
//! design decision (v2 sec. 6): `LocalBackend` and `ManualNotebookBackend`.
//! Both drive `scripts/training/train_expert.py`; the command is injectable so
//! tests can use a fake script.

use super::jobs::TrainingJob;
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::process::{Child, Command};

pub const AUDIT_PASS_STATUSES: [&str; 4] = ["passed", "pass", "approved", "ok"];

/// Privacy facts about a bundle, read from `bundle_manifest.json` and
/// `anonymization_audit.json`. Absent fields stay `None`/`false` and make
/// remote-style backends refuse (fail closed).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct BundleManifest {
    pub privacy_level: Option<String>,
    pub local_only: Option<bool>,
    pub audit_passed: bool,
}

impl BundleManifest {
    pub fn load(bundle_dir: &Path) -> Result<Self> {
        let mpath = bundle_dir.join("bundle_manifest.json");
        let m: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&mpath).with_context(|| format!("reading {}", mpath.display()))?,
        )
        .with_context(|| format!("parsing {}", mpath.display()))?;
        let audit_passed = match std::fs::read(bundle_dir.join("anonymization_audit.json")) {
            Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
                .map(|a| {
                    a.get("passed").and_then(|v| v.as_bool()) == Some(true)
                        || a.get("status")
                            .and_then(|v| v.as_str())
                            .map(|s| AUDIT_PASS_STATUSES.contains(&s.to_lowercase().as_str()))
                            .unwrap_or(false)
                })
                .unwrap_or(false),
            Err(_) => false,
        };
        Ok(Self {
            privacy_level: m
                .get("privacy_level")
                .and_then(|v| v.as_str())
                .map(|s| s.to_uppercase()),
            local_only: m.get("local_only").and_then(|v| v.as_bool()),
            audit_passed,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobPhase {
    Running,
    AwaitingArtifact,
    Succeeded,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifacts {
    pub gguf: PathBuf,
    pub report: Option<PathBuf>,
}

#[async_trait]
pub trait ComputeBackend: Send + Sync {
    fn name(&self) -> &'static str;
    /// Privacy gate: may this bundle be handled by this backend?
    fn accepts(&self, bundle: &BundleManifest) -> bool;
    async fn submit(&self, job: &TrainingJob) -> Result<RemoteRef>;
    async fn poll(&self, r: &RemoteRef) -> Result<JobPhase>;
    /// Moves/locates the produced GGUF (+ train_report.json) into `dst`.
    async fn fetch_artifacts(&self, r: &RemoteRef, dst: &Path) -> Result<Artifacts>;
    async fn cancel(&self, r: &RemoteRef) -> Result<()>;
}

pub fn job_dir(jobs_root: &Path, id: &str) -> PathBuf {
    jobs_root.join(id)
}

pub fn log_path(jobs_root: &Path, id: &str) -> PathBuf {
    job_dir(jobs_root, id).join("logs.txt")
}

fn find_file(dir: &Path, pred: &dyn Fn(&Path) -> bool, depth: u32) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in &entries {
        let p = e.path();
        if p.is_file() && pred(&p) {
            return Some(p);
        }
    }
    if depth > 0 {
        for e in &entries {
            let p = e.path();
            if p.is_dir() {
                if let Some(f) = find_file(&p, pred, depth - 1) {
                    return Some(f);
                }
            }
        }
    }
    None
}

fn find_artifacts(dir: &Path) -> Option<Artifacts> {
    let gguf = find_file(
        dir,
        &|p| p.extension().map(|e| e == "gguf").unwrap_or(false),
        3,
    )?;
    let report = find_file(
        dir,
        &|p| {
            p.file_name()
                .map(|n| n == "train_report.json")
                .unwrap_or(false)
        },
        3,
    );
    Some(Artifacts { gguf, report })
}

fn move_into(src: &Path, dst_dir: &Path) -> Result<PathBuf> {
    let target = dst_dir.join(
        src.file_name()
            .ok_or_else(|| anyhow!("bad artifact path"))?,
    );
    if target != src {
        std::fs::create_dir_all(dst_dir)?;
        std::fs::rename(src, &target)
            .or_else(|_| std::fs::copy(src, &target).map(|_| ()))
            .with_context(|| format!("moving {} -> {}", src.display(), target.display()))?;
    }
    Ok(target)
}

fn script_args(command: &[String], job: &TrainingJob, backend: &str, out: &Path) -> Command {
    let mut c = Command::new(&command[0]);
    c.args(&command[1..])
        .arg("--bundle")
        .arg(&job.bundle_path)
        .arg("--base-model")
        .arg(&job.base_model)
        .arg("--out")
        .arg(out)
        .arg("--backend")
        .arg(backend);
    c
}

fn open_log(jobs_root: &Path, id: &str) -> Result<(std::fs::File, std::fs::File)> {
    std::fs::create_dir_all(job_dir(jobs_root, id))?;
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(jobs_root, id))?;
    Ok((f.try_clone()?, f))
}

fn map_exit(code: Option<i32>) -> JobPhase {
    match code {
        Some(0) => JobPhase::Succeeded,
        Some(2) => JobPhase::Failed("trainer gated or failed (exit 2); see logs".into()),
        Some(c) => JobPhase::Failed(format!("trainer exited with code {c}; see logs")),
        None => JobPhase::Failed("trainer terminated by signal".into()),
    }
}

enum Entry {
    Running(Child),
    Done(JobPhase),
}

/// Runs the trainer as a child process on this machine. Always allowed.
pub struct LocalBackend {
    command: Vec<String>,
    jobs_root: PathBuf,
    procs: Arc<Mutex<HashMap<String, Entry>>>,
}

impl LocalBackend {
    /// `command` = program + leading args, e.g. `["python3", "scripts/training/train_expert.py"]`.
    pub fn new(command: Vec<String>, jobs_root: PathBuf) -> Self {
        assert!(!command.is_empty(), "empty trainer command");
        Self {
            command,
            jobs_root,
            procs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ComputeBackend for LocalBackend {
    fn name(&self) -> &'static str {
        "local"
    }

    fn accepts(&self, _bundle: &BundleManifest) -> bool {
        true
    }

    async fn submit(&self, job: &TrainingJob) -> Result<RemoteRef> {
        let (out, err) = open_log(&self.jobs_root, &job.id)?;
        let out_dir = job_dir(&self.jobs_root, &job.id).join("out");
        let child = script_args(&self.command, job, "local", &out_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawning {:?}", self.command))?;
        self.procs
            .lock()
            .map_err(|_| anyhow!("poisoned"))?
            .insert(job.id.clone(), Entry::Running(child));
        Ok(RemoteRef(job.id.clone()))
    }

    async fn poll(&self, r: &RemoteRef) -> Result<JobPhase> {
        let mut procs = self.procs.lock().map_err(|_| anyhow!("poisoned"))?;
        let phase = match procs.get_mut(&r.0) {
            None => JobPhase::Failed("process lost (daemon restarted?)".into()),
            Some(Entry::Done(p)) => p.clone(),
            Some(Entry::Running(child)) => match child.try_wait()? {
                None => return Ok(JobPhase::Running),
                Some(st) => map_exit(st.code()),
            },
        };
        procs.insert(r.0.clone(), Entry::Done(phase.clone()));
        Ok(phase)
    }

    async fn fetch_artifacts(&self, r: &RemoteRef, dst: &Path) -> Result<Artifacts> {
        let a = find_artifacts(&job_dir(&self.jobs_root, &r.0).join("out"))
            .ok_or_else(|| anyhow!("no .gguf artifact produced"))?;
        Ok(Artifacts {
            gguf: move_into(&a.gguf, dst)?,
            report: match a.report {
                Some(p) => Some(move_into(&p, dst)?),
                None => None,
            },
        })
    }

    async fn cancel(&self, r: &RemoteRef) -> Result<()> {
        let mut procs = self.procs.lock().map_err(|_| anyhow!("poisoned"))?;
        if let Some(Entry::Running(child)) = procs.get_mut(&r.0) {
            child.start_kill()?;
        }
        procs.insert(r.0.clone(), Entry::Done(JobPhase::Cancelled));
        Ok(())
    }
}

/// Generates the Colab notebook + bundle.zip, then waits for the human to
/// upload the GGUF. Refuses anything not explicitly safe to leave the node.
pub struct ManualNotebookBackend {
    command: Vec<String>,
    jobs_root: PathBuf,
}

impl ManualNotebookBackend {
    pub fn new(command: Vec<String>, jobs_root: PathBuf) -> Self {
        assert!(!command.is_empty(), "empty trainer command");
        Self { command, jobs_root }
    }

    pub fn artifacts_dir(&self, id: &str) -> PathBuf {
        job_dir(&self.jobs_root, id).join("artifacts")
    }
}

#[async_trait]
impl ComputeBackend for ManualNotebookBackend {
    fn name(&self) -> &'static str {
        "notebook"
    }

    /// Fail closed: needs an explicit `local_only: false`, a known non-P4
    /// privacy level and a passed anonymization audit.
    fn accepts(&self, b: &BundleManifest) -> bool {
        b.local_only == Some(false)
            && b.audit_passed
            && matches!(b.privacy_level.as_deref(), Some(l) if l != "P4")
    }

    async fn submit(&self, job: &TrainingJob) -> Result<RemoteRef> {
        let manifest = BundleManifest::load(Path::new(&job.bundle_path))?;
        if !self.accepts(&manifest) {
            bail!("REFUSED: bundle is local_only or lacks a passed anonymization audit");
        }
        let (out, err) = open_log(&self.jobs_root, &job.id)?;
        let out_dir = job_dir(&self.jobs_root, &job.id).join("notebook");
        let st = script_args(&self.command, job, "notebook", &out_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .status()
            .await
            .with_context(|| format!("spawning {:?}", self.command))?;
        if !st.success() {
            bail!(
                "notebook generation failed (exit {:?}); see logs",
                st.code()
            );
        }
        std::fs::create_dir_all(self.artifacts_dir(&job.id))?;
        Ok(RemoteRef(job.id.clone()))
    }

    async fn poll(&self, r: &RemoteRef) -> Result<JobPhase> {
        let dir = self.artifacts_dir(&r.0);
        Ok(match find_artifacts(&dir) {
            Some(a) if a.report.is_some() => JobPhase::Succeeded,
            _ => JobPhase::AwaitingArtifact,
        })
    }

    async fn fetch_artifacts(&self, r: &RemoteRef, dst: &Path) -> Result<Artifacts> {
        let a = find_artifacts(&self.artifacts_dir(&r.0))
            .ok_or_else(|| anyhow!("artifacts not uploaded yet"))?;
        Ok(Artifacts {
            gguf: move_into(&a.gguf, dst)?,
            report: match a.report {
                Some(p) => Some(move_into(&p, dst)?),
                None => None,
            },
        })
    }

    async fn cancel(&self, _r: &RemoteRef) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    pub fn write_bundle(dir: &Path, manifest: &str, audit: Option<&str>) -> PathBuf {
        let b = dir.join("bundle");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("bundle_manifest.json"), manifest).unwrap();
        std::fs::write(b.join("train.jsonl"), "{}\n").unwrap();
        std::fs::write(b.join("eval.jsonl"), "{}\n").unwrap();
        if let Some(a) = audit {
            std::fs::write(b.join("anonymization_audit.json"), a).unwrap();
        }
        b
    }

    /// Fake trainer. Parses `--out`; behaviour chosen by `body`.
    pub fn write_script(dir: &Path, body: &str) -> Vec<String> {
        let p = dir.join("fake_train.sh");
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --out) OUT=\"$2\";; --backend) BE=\"$2\";; esac; shift; done\nmkdir -p \"$OUT\"\n{body}\n"
            ),
        )
        .unwrap();
        vec!["sh".into(), p.to_string_lossy().into_owned()]
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::training::jobs::{JobRepository, NewJob};

    fn job(repo: &JobRepository, bundle: &Path, backend: &str) -> TrainingJob {
        repo.create(NewJob {
            owner: "root".into(),
            domain: "d".into(),
            bundle_path: bundle.to_string_lossy().into(),
            bundle_hash: "h".into(),
            backend: backend.into(),
            base_model: "m".into(),
        })
        .unwrap()
    }

    async fn wait_done(b: &dyn ComputeBackend, r: &RemoteRef) -> JobPhase {
        for _ in 0..200 {
            let p = b.poll(r).await.unwrap();
            if p != JobPhase::Running {
                return p;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("job never finished");
    }

    #[tokio::test]
    async fn local_success_failure_and_cancel() {
        let t = tempfile::tempdir().unwrap();
        let repo = JobRepository::open_in_memory().unwrap();
        let bundle = write_bundle(
            t.path(),
            r#"{"privacy_level":"P4","local_only":true}"#,
            None,
        );
        let jobs = t.path().join("jobs");

        let ok = LocalBackend::new(
            write_script(t.path(), "echo training; printf GGUFxxxx > \"$OUT/m.gguf\"; echo '{}' > \"$OUT/train_report.json\""),
            jobs.clone(),
        );
        assert!(ok.accepts(&BundleManifest::load(&bundle).unwrap())); // local always allowed
        let j = job(&repo, &bundle, "local");
        let r = ok.submit(&j).await.unwrap();
        assert_eq!(wait_done(&ok, &r).await, JobPhase::Succeeded);
        let a = ok.fetch_artifacts(&r, &t.path().join("dst")).await.unwrap();
        assert!(a.gguf.is_file() && a.report.is_some());
        let log = std::fs::read_to_string(log_path(&jobs, &j.id)).unwrap();
        assert!(log.contains("training"));

        let bad = LocalBackend::new(
            write_script(t.path(), "echo gated >&2; exit 2"),
            jobs.clone(),
        );
        let j2 = job(&repo, &bundle, "local");
        let r2 = bad.submit(&j2).await.unwrap();
        assert!(matches!(wait_done(&bad, &r2).await, JobPhase::Failed(m) if m.contains("exit 2")));
        assert!(std::fs::read_to_string(log_path(&jobs, &j2.id))
            .unwrap()
            .contains("gated"));

        let slow = LocalBackend::new(write_script(t.path(), "sleep 30"), jobs.clone());
        let j3 = job(&repo, &bundle, "local");
        let r3 = slow.submit(&j3).await.unwrap();
        assert_eq!(slow.poll(&r3).await.unwrap(), JobPhase::Running);
        slow.cancel(&r3).await.unwrap();
        assert_eq!(slow.poll(&r3).await.unwrap(), JobPhase::Cancelled);
    }

    #[tokio::test]
    async fn notebook_refuses_unaudited_and_local_only() {
        let t = tempfile::tempdir().unwrap();
        let repo = JobRepository::open_in_memory().unwrap();
        let nb = ManualNotebookBackend::new(
            write_script(t.path(), "echo nb > \"$OUT/train_expert_colab.ipynb\""),
            t.path().join("jobs"),
        );
        let cases = [
            (r#"{"privacy_level":"P3","local_only":false}"#, None), // no audit
            (
                r#"{"privacy_level":"P3","local_only":false}"#,
                Some(r#"{"passed":false}"#),
            ),
            (
                r#"{"privacy_level":"P3","local_only":true}"#,
                Some(r#"{"passed":true}"#),
            ),
            (
                r#"{"privacy_level":"P4","local_only":false}"#,
                Some(r#"{"passed":true}"#),
            ),
            (r#"{"privacy_level":"P3"}"#, Some(r#"{"passed":true}"#)), // local_only absent
            (r#"{"local_only":false}"#, Some(r#"{"passed":true}"#)),   // level absent
        ];
        for (i, (m, a)) in cases.iter().enumerate() {
            let d = t.path().join(format!("c{i}"));
            let b = write_bundle(&d, m, *a);
            assert!(
                !nb.accepts(&BundleManifest::load(&b).unwrap()),
                "case {i} must be refused"
            );
            let j = job(&repo, &b, "notebook");
            assert!(
                nb.submit(&j).await.is_err(),
                "case {i} submit must be refused"
            );
        }
        let b = write_bundle(
            &t.path().join("good"),
            r#"{"privacy_level":"P3","local_only":false}"#,
            Some(r#"{"status":"approved"}"#),
        );
        assert!(nb.accepts(&BundleManifest::load(&b).unwrap()));
        let j = job(&repo, &b, "notebook");
        let r = nb.submit(&j).await.unwrap();
        assert_eq!(nb.poll(&r).await.unwrap(), JobPhase::AwaitingArtifact);
        let dir = nb.artifacts_dir(&j.id);
        std::fs::write(dir.join("m.gguf"), b"GGUF").unwrap();
        assert_eq!(nb.poll(&r).await.unwrap(), JobPhase::AwaitingArtifact); // report missing
        std::fs::write(dir.join("train_report.json"), b"{}").unwrap();
        assert_eq!(nb.poll(&r).await.unwrap(), JobPhase::Succeeded);
    }
}
