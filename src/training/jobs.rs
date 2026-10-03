//! SQLite-backed `training_jobs` repository.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    AwaitingArtifact,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::Running => "running",
            JobStatus::AwaitingArtifact => "awaiting_artifact",
            JobStatus::Succeeded => "succeeded",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "queued" => JobStatus::Queued,
            "running" => JobStatus::Running,
            "awaiting_artifact" => JobStatus::AwaitingArtifact,
            "succeeded" => JobStatus::Succeeded,
            "failed" => JobStatus::Failed,
            "cancelled" => JobStatus::Cancelled,
            other => return Err(anyhow!("unknown job status {other:?}")),
        })
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrainingJob {
    pub id: String,
    pub owner: String,
    pub domain: String,
    pub bundle_path: String,
    pub bundle_hash: String,
    pub backend: String,
    pub base_model: String,
    pub status: JobStatus,
    pub logs_path: Option<String>,
    pub artifact_path: Option<String>,
    pub metrics_json: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub owner: String,
    pub domain: String,
    pub bundle_path: String,
    pub bundle_hash: String,
    pub backend: String,
    pub base_model: String,
}

/// Partial update; `None` leaves the column unchanged.
#[derive(Debug, Clone, Default)]
pub struct JobUpdate {
    pub status: Option<JobStatus>,
    pub logs_path: Option<String>,
    pub artifact_path: Option<String>,
    pub metrics_json: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct JobRepository {
    conn: Arc<Mutex<Connection>>,
}

const COLS: &str = "id, owner, domain, bundle_path, bundle_hash, backend, base_model, status, \
                    logs_path, artifact_path, metrics_json, error, created_at, updated_at";

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn row_to_job(r: &Row<'_>) -> rusqlite::Result<TrainingJob> {
    let status: String = r.get(7)?;
    Ok(TrainingJob {
        id: r.get(0)?,
        owner: r.get(1)?,
        domain: r.get(2)?,
        bundle_path: r.get(3)?,
        bundle_hash: r.get(4)?,
        backend: r.get(5)?,
        base_model: r.get(6)?,
        status: JobStatus::parse(&status).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, e.into())
        })?,
        logs_path: r.get(8)?,
        artifact_path: r.get(9)?,
        metrics_json: r.get(10)?,
        error: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
    })
}

impl JobRepository {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(db_path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS training_jobs (
                id TEXT PRIMARY KEY,
                owner TEXT NOT NULL,
                domain TEXT NOT NULL,
                bundle_path TEXT NOT NULL,
                bundle_hash TEXT NOT NULL,
                backend TEXT NOT NULL,
                base_model TEXT NOT NULL,
                status TEXT NOT NULL,
                logs_path TEXT,
                artifact_path TEXT,
                metrics_json TEXT,
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_training_jobs_created ON training_jobs(created_at);",
        )?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn create(&self, n: NewJob) -> Result<TrainingJob> {
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now();
        {
            let c = self.conn.lock().map_err(|_| anyhow!("jobs db poisoned"))?;
            c.execute(
                "INSERT INTO training_jobs (id, owner, domain, bundle_path, bundle_hash, backend, \
                 base_model, status, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?8)",
                params![
                    id,
                    n.owner,
                    n.domain,
                    n.bundle_path,
                    n.bundle_hash,
                    n.backend,
                    n.base_model,
                    ts
                ],
            )?;
        }
        self.get(&id)?
            .ok_or_else(|| anyhow!("job vanished after insert"))
    }

    pub fn get(&self, id: &str) -> Result<Option<TrainingJob>> {
        let c = self.conn.lock().map_err(|_| anyhow!("jobs db poisoned"))?;
        Ok(c.query_row(
            &format!("SELECT {COLS} FROM training_jobs WHERE id = ?1"),
            params![id],
            row_to_job,
        )
        .optional()?)
    }

    /// Newest first.
    pub fn list(&self, limit: usize) -> Result<Vec<TrainingJob>> {
        let c = self.conn.lock().map_err(|_| anyhow!("jobs db poisoned"))?;
        let mut st = c.prepare(&format!(
            "SELECT {COLS} FROM training_jobs ORDER BY created_at DESC, rowid DESC LIMIT ?1"
        ))?;
        let rows = st.query_map(params![limit as i64], row_to_job)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Applies `u` only while the job is not in a terminal state, so a late
    /// monitor task can never overwrite `cancelled`. Returns whether it applied.
    pub fn update(&self, id: &str, u: JobUpdate) -> Result<bool> {
        let c = self.conn.lock().map_err(|_| anyhow!("jobs db poisoned"))?;
        let n = c.execute(
            "UPDATE training_jobs SET \
                status = COALESCE(?2, status), \
                logs_path = COALESCE(?3, logs_path), \
                artifact_path = COALESCE(?4, artifact_path), \
                metrics_json = COALESCE(?5, metrics_json), \
                error = COALESCE(?6, error), \
                updated_at = ?7 \
             WHERE id = ?1 AND status NOT IN ('succeeded','failed','cancelled')",
            params![
                id,
                u.status.map(|s| s.as_str()),
                u.logs_path,
                u.artifact_path,
                u.metrics_json,
                u.error,
                now()
            ],
        )?;
        Ok(n > 0)
    }

    /// Marks a non-terminal job cancelled. Returns false if already terminal.
    pub fn cancel(&self, id: &str) -> Result<bool> {
        self.update(
            id,
            JobUpdate {
                status: Some(JobStatus::Cancelled),
                ..Default::default()
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_job() -> NewJob {
        NewJob {
            owner: "root".into(),
            domain: "legal".into(),
            bundle_path: "/b".into(),
            bundle_hash: "h".into(),
            backend: "local".into(),
            base_model: "m".into(),
        }
    }

    #[test]
    fn crud_and_terminal_guard() {
        let dir = tempfile::tempdir().unwrap();
        let repo = JobRepository::open(&dir.path().join("t.sqlite3")).unwrap();
        let j = repo.create(new_job()).unwrap();
        assert_eq!(j.status, JobStatus::Queued);
        assert!(repo
            .update(
                &j.id,
                JobUpdate {
                    status: Some(JobStatus::Running),
                    ..Default::default()
                }
            )
            .unwrap());
        assert_eq!(repo.list(10).unwrap().len(), 1);
        assert!(repo.cancel(&j.id).unwrap());
        // terminal: further updates and cancels are no-ops
        assert!(!repo
            .update(
                &j.id,
                JobUpdate {
                    status: Some(JobStatus::Succeeded),
                    ..Default::default()
                }
            )
            .unwrap());
        assert!(!repo.cancel(&j.id).unwrap());
        assert_eq!(
            repo.get(&j.id).unwrap().unwrap().status,
            JobStatus::Cancelled
        );
        assert!(repo.get("nope").unwrap().is_none());
    }
}
