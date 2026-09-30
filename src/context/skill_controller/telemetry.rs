//! Skill usage telemetry: privacy-preserving lifecycle events (D12, EP-60).
//!
//! Events are appended to the **existing** workspace database through the shared
//! migration framework (`crate::storage::migrations`, v6), never to a second
//! database. Only bounded opaque identifiers are stored: no task text, absolute
//! path, memory body or credential reaches the table, a log line or an error.
//!
//! Stages distinguish `selected`, `delivered`, `invoked` and `completed`; only
//! the last two come from a harness acknowledgment, and a repeated `event_id`
//! is idempotent. Fail-open: [`TelemetryWriter::record`] and
//! [`TelemetryWriter::prune`] return `Result` and never panic, and both take
//! `dry_run` so the default call reports the plan without applying it.

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::Connection;
use thiserror::Error;

/// Documented default retention window, in days (D12).
pub const DEFAULT_RETENTION_DAYS: u32 = 30;

/// Maximum length of any stored identifier.
pub const MAX_FIELD_LEN: usize = 128;

const _: () = assert!(DEFAULT_RETENTION_DAYS > 0 && MAX_FIELD_LEN > 0);

/// Fixed-width UTC stamp: `observed_at` then sorts lexicographically, so the
/// retention sweep compares TEXT with a plain `<`.
const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// Failures name the offending field only — never its value, so no untrusted
/// text can reach an error string.
#[derive(Debug, Error, PartialEq)]
pub enum TelemetryError {
    #[error("skill usage telemetry database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("{field} must be 1..={max} characters")]
    FieldLength { field: &'static str, max: usize },
    #[error("{field} is not an opaque identifier: no text, spaces or path separators")]
    NotOpaque { field: &'static str },
    #[error("a retention window of {days} days is out of range")]
    RetentionOutOfRange { days: u32 },
}

/// Lifecycle stage of a dispatched skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillStage {
    /// Chosen by the controller for a task.
    Selected,
    /// Reached a harness (published file or MCP context pack).
    Delivered,
    /// Acknowledged by the harness as invoked.
    Invoked,
    /// Acknowledged by the harness as completed.
    Completed,
}

impl SkillStage {
    /// Stable storage and log representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::Delivered => "delivered",
            Self::Invoked => "invoked",
            Self::Completed => "completed",
        }
    }
}

/// How long events are kept: configurable, and retention can be disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicy {
    /// Keep every event; [`TelemetryWriter::prune`] deletes nothing.
    KeepForever,
    /// Delete events observed more than `max_age_days` before the sweep.
    Expire { max_age_days: u32 },
}

impl RetentionPolicy {
    /// The documented default: 30 days.
    pub const fn default_policy() -> Self {
        Self::Expire {
            max_age_days: DEFAULT_RETENTION_DAYS,
        }
    }

    /// Oldest stamp the policy keeps, or `None` when retention is disabled.
    fn cutoff(self, now: DateTime<Utc>) -> Result<Option<String>, TelemetryError> {
        let Self::Expire { max_age_days } = self else {
            return Ok(None);
        };
        let out_of_range = || TelemetryError::RetentionOutOfRange { days: max_age_days };
        let age = TimeDelta::try_days(i64::from(max_age_days)).ok_or_else(out_of_range)?;
        let cutoff = now.checked_sub_signed(age).ok_or_else(out_of_range)?;
        Ok(Some(cutoff.format(TIMESTAMP_FORMAT).to_string()))
    }
}

/// One lifecycle event. Every text field is an opaque identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEvent {
    /// Idempotency key for one (skill, stage) observation.
    pub event_id: String,
    /// Opaque workspace identifier.
    pub workspace_id: String,
    /// Opaque task identifier, never the task text.
    pub task_id: String,
    /// Harness the event came from, for example `claude-code`.
    pub tool: String,
    /// Skill name from the registry.
    pub skill_name: String,
    /// Package hash, when the publisher recorded one.
    pub package_hash: Option<String>,
    /// Lifecycle stage.
    pub stage: SkillStage,
    /// Bounded outcome label, for example `ok`.
    pub outcome: String,
    /// Measured latency, when the harness reported one.
    pub latency_ms: Option<i64>,
    /// Token estimate, when the harness reported one.
    pub token_estimate: Option<i64>,
    /// When the observation happened.
    pub observed_at: DateTime<Utc>,
}

impl UsageEvent {
    /// Event with the required opaque identifiers and a default `ok` outcome.
    pub fn new(
        event_id: impl Into<String>,
        workspace_id: impl Into<String>,
        task_id: impl Into<String>,
        tool: impl Into<String>,
        skill_name: impl Into<String>,
        stage: SkillStage,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            workspace_id: workspace_id.into(),
            task_id: task_id.into(),
            tool: tool.into(),
            skill_name: skill_name.into(),
            package_hash: None,
            stage,
            outcome: "ok".to_string(),
            latency_ms: None,
            token_estimate: None,
            observed_at: Utc::now(),
        }
    }

    /// Reject anything that is not a short opaque identifier, so task text, an
    /// absolute path or a credential can never be stored or logged.
    fn validate(&self) -> Result<(), TelemetryError> {
        check_opaque("event_id", &self.event_id)?;
        check_opaque("workspace_id", &self.workspace_id)?;
        check_opaque("task_id", &self.task_id)?;
        check_opaque("tool", &self.tool)?;
        check_opaque("skill_name", &self.skill_name)?;
        check_opaque("outcome", &self.outcome)?;
        if let Some(package_hash) = &self.package_hash {
            check_opaque("package_hash", package_hash)?;
        }
        Ok(())
    }
}

/// Accept only `[A-Za-z0-9_.:-]` identifiers of at most [`MAX_FIELD_LEN`] bytes.
fn check_opaque(field: &'static str, value: &str) -> Result<(), TelemetryError> {
    if !(1..=MAX_FIELD_LEN).contains(&value.len()) {
        return Err(TelemetryError::FieldLength {
            field,
            max: MAX_FIELD_LEN,
        });
    }
    let opaque = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'));
    if !opaque {
        return Err(TelemetryError::NotOpaque { field });
    }
    Ok(())
}

/// Row counts are never negative; keep the conversion total.
fn affected_rows<T: TryInto<u64>>(value: T) -> u64 {
    value.try_into().unwrap_or(0)
}

/// Result of a (possibly dry-run) write or retention sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    /// False when the call was a dry run: the plan was reported, nothing applied.
    pub applied: bool,
    /// Rows inserted (`record`: `0` means the event ID already existed) or deleted.
    pub affected: u64,
}

/// Bounded, fail-open writer over the workspace telemetry table.
///
/// The writer never creates a database and never owns the connection: the caller
/// supplies the existing workspace connection, so telemetry shares the
/// workspace schema and its transaction semantics.
pub struct TelemetryWriter<'a> {
    conn: &'a Connection,
    retention: RetentionPolicy,
}

impl<'a> TelemetryWriter<'a> {
    /// Writer with the documented 30-day retention.
    pub fn new(conn: &'a Connection) -> Self {
        Self::with_retention(conn, RetentionPolicy::default_policy())
    }

    /// Writer with an explicit retention policy.
    pub const fn with_retention(conn: &'a Connection, retention: RetentionPolicy) -> Self {
        Self { conn, retention }
    }

    /// Ensure the telemetry table exists, replaying the v6 DDL idempotently.
    ///
    /// Needed because a legacy database is backfilled to v6 by `schema_migrations`
    /// bookkeeping without executing the v6 DDL.
    pub fn ensure_schema(&self) -> Result<(), TelemetryError> {
        crate::storage::migrations::ensure_skill_usage_events_schema(self.conn)?;
        Ok(())
    }

    /// Append one event. A repeated `event_id` is idempotent, not an error.
    pub fn record(&self, event: &UsageEvent, dry_run: bool) -> Result<Outcome, TelemetryError> {
        event.validate()?;
        if dry_run {
            return Ok(Outcome {
                applied: false,
                affected: 0,
            });
        }
        let affected = self.conn.execute(
            "INSERT OR IGNORE INTO skill_usage_events (
                 event_id, workspace_id, task_id, tool, skill_name, package_hash,
                 stage, outcome, latency_ms, token_estimate, observed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                event.event_id,
                event.workspace_id,
                event.task_id,
                event.tool,
                event.skill_name,
                event.package_hash,
                event.stage.as_str(),
                event.outcome,
                event.latency_ms,
                event.token_estimate,
                event.observed_at.format(TIMESTAMP_FORMAT).to_string(),
            ],
        )?;
        tracing::debug!(
            event_id = %event.event_id,
            stage = event.stage.as_str(),
            affected,
            "skill usage event stored"
        );
        Ok(Outcome {
            applied: true,
            affected: affected_rows(affected),
        })
    }

    /// Delete events observed before the retention cutoff, and only those.
    pub fn prune(&self, now: DateTime<Utc>, dry_run: bool) -> Result<Outcome, TelemetryError> {
        let Some(cutoff) = self.retention.cutoff(now)? else {
            return Ok(Outcome {
                applied: !dry_run,
                affected: 0,
            });
        };
        let expired = self.conn.query_row(
            "SELECT count(*) FROM skill_usage_events WHERE observed_at < ?1",
            [&cutoff],
            |row| row.get::<_, i64>(0),
        )?;
        if dry_run {
            return Ok(Outcome {
                applied: false,
                affected: affected_rows(expired),
            });
        }
        let deleted = self.conn.execute(
            "DELETE FROM skill_usage_events WHERE observed_at < ?1",
            [&cutoff],
        )?;
        tracing::debug!(deleted, "skill usage telemetry pruned");
        Ok(Outcome {
            applied: true,
            affected: affected_rows(deleted),
        })
    }

    /// Number of stored events.
    pub fn count(&self) -> Result<u64, TelemetryError> {
        let total = self
            .conn
            .query_row("SELECT count(*) FROM skill_usage_events", [], |row| {
                row.get::<_, i64>(0)
            })?;
        Ok(affected_rows(total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    const WORKSPACE: &str = "swal/xavier/instance";
    const TASK: &str = "task-opaque-0001";
    const TOOL: &str = "claude-code";
    const SKILL: &str = "xavier-cognitive-memory";
    const NOW: &str = "2026-02-01T00:00:00.000Z";

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(NOW)
            .expect("fixture timestamp is valid")
            .with_timezone(&Utc)
    }

    fn days_ago(days: i64) -> DateTime<Utc> {
        now() - TimeDelta::try_days(days).expect("day count is in range")
    }

    fn event(event_id: &str, stage: SkillStage) -> UsageEvent {
        let mut event = UsageEvent::new(event_id, WORKSPACE, TASK, TOOL, SKILL, stage);
        event.package_hash = Some("sha256-abcdef0123".to_string());
        event.observed_at = now();
        event
    }

    /// Isolated in-memory workspace database with every baseline migration applied.
    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        crate::storage::migrations::run(&conn).expect("baseline migrations must apply");
        conn
    }

    fn writer(conn: &Connection) -> TelemetryWriter<'_> {
        let writer = TelemetryWriter::new(conn);
        writer
            .ensure_schema()
            .expect("telemetry schema must be available");
        writer
    }

    /// Read a stored row back, as the coverage layer will.
    fn stored_row(
        conn: &Connection,
        event_id: &str,
    ) -> Option<(String, Option<String>, Option<i64>)> {
        conn.query_row(
            "SELECT stage, package_hash, latency_ms FROM skill_usage_events WHERE event_id = ?1",
            [event_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()
    }

    fn event_ids(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT event_id FROM skill_usage_events ORDER BY event_id")
            .expect("prepare");
        stmt.query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect()
    }

    /// Insert one aged event, as a harness would have days ago.
    fn seed_with_age(conn: &Connection, event_id: &str, days_old: i64) {
        let mut event = event(event_id, SkillStage::Invoked);
        event.observed_at = days_ago(days_old);
        writer(conn)
            .record(&event, false)
            .expect("seed event must insert");
    }

    #[test]
    fn every_stage_round_trips_through_the_schema() {
        let conn = migrated();
        let writer = writer(&conn);
        for stage in [
            SkillStage::Selected,
            SkillStage::Delivered,
            SkillStage::Invoked,
            SkillStage::Completed,
        ] {
            let mut event = event(&format!("evt-{}", stage.as_str()), stage);
            event.latency_ms = Some(12);
            event.token_estimate = Some(340);
            let outcome = writer.record(&event, false).expect("record must succeed");
            assert!(
                outcome.applied && outcome.affected == 1,
                "stage {} must insert one row",
                stage.as_str()
            );

            let (stored_stage, package_hash, latency_ms) = stored_row(&conn, &event.event_id)
                .unwrap_or_else(|| panic!("stage {} must be readable", stage.as_str()));
            assert_eq!(stored_stage, stage.as_str(), "stage must round-trip");
            assert_eq!(package_hash, event.package_hash, "hash must round-trip");
            assert_eq!(latency_ms, event.latency_ms, "latency must round-trip");
        }
        assert_eq!(writer.count().expect("count must succeed"), 4);
    }

    #[test]
    fn duplicate_event_id_is_idempotent() {
        let conn = migrated();
        let writer = writer(&conn);
        let event = event("evt-duplicate", SkillStage::Delivered);

        let first = writer
            .record(&event, false)
            .expect("first record must succeed");
        let second = writer
            .record(&event, false)
            .expect("replayed record must succeed");
        assert!(
            first.applied && first.affected == 1,
            "the first write inserts a row"
        );
        assert!(second.applied, "a replay is still a successful write");
        assert_eq!(second.affected, 0, "a replay must not insert a second row");
        assert_eq!(writer.count().expect("count must succeed"), 1);
    }

    #[test]
    fn record_dry_run_reports_the_plan_without_writing() {
        let conn = migrated();
        let writer = writer(&conn);
        let event = event("evt-dry-run", SkillStage::Selected);

        let outcome = writer
            .record(&event, true)
            .expect("a dry run must not fail");
        assert!(
            !outcome.applied && outcome.affected == 0,
            "a dry run reports that nothing was applied"
        );
        assert_eq!(writer.count().expect("count must succeed"), 0);
        assert!(
            stored_row(&conn, "evt-dry-run").is_none(),
            "a dry run must not write the row"
        );
    }

    #[test]
    fn prune_dry_run_reports_the_plan_without_deleting() {
        let conn = migrated();
        let writer = writer(&conn);
        seed_with_age(&conn, "evt-old", 45);
        seed_with_age(&conn, "evt-fresh", 1);

        let outcome = writer.prune(now(), true).expect("a dry run must not fail");
        assert!(
            !outcome.applied,
            "a dry run reports that nothing was applied"
        );
        assert_eq!(outcome.affected, 1, "the dry run reports the expired row");
        assert_eq!(
            writer.count().expect("count must succeed"),
            2,
            "a dry run must not delete"
        );
    }

    #[test]
    fn prune_removes_expired_events_only() {
        let conn = migrated();
        let writer = writer(&conn);
        seed_with_age(&conn, "evt-ancient", 400);
        seed_with_age(&conn, "evt-expired", 31);
        seed_with_age(&conn, "evt-edge", 30);
        seed_with_age(&conn, "evt-fresh", 29);

        let outcome = writer.prune(now(), false).expect("prune must succeed");
        assert!(outcome.applied, "the sweep applied");
        assert_eq!(
            outcome.affected, 2,
            "only rows observed more than 30 days ago are expired"
        );
        assert_eq!(
            event_ids(&conn),
            vec!["evt-edge", "evt-fresh"],
            "the 30-day boundary row and newer rows must survive"
        );
    }

    #[test]
    fn retention_is_configurable_and_can_be_disabled() {
        let conn = migrated();
        seed_with_age(&conn, "evt-ancient", 400);
        seed_with_age(&conn, "evt-fresh", 1);

        let short =
            TelemetryWriter::with_retention(&conn, RetentionPolicy::Expire { max_age_days: 2 });
        let swept = short
            .prune(now(), false)
            .expect("short retention must sweep");
        assert_eq!(
            swept.affected, 1,
            "a 2-day window expires only the ancient row"
        );

        seed_with_age(&conn, "evt-ancient", 400);
        let forever = TelemetryWriter::with_retention(&conn, RetentionPolicy::KeepForever);
        let kept = forever
            .prune(now(), false)
            .expect("disabled retention must sweep");
        assert!(kept.applied, "a disabled sweep still runs as a no-op");
        assert_eq!(kept.affected, 0, "disabled retention deletes nothing");
        assert_eq!(forever.count().expect("count must succeed"), 2);
    }

    #[test]
    fn untrusted_task_text_and_absolute_paths_are_rejected() {
        let conn = migrated();
        let writer = writer(&conn);
        let cases: [(&str, fn(&mut UsageEvent)); 4] = [
            ("task_id carries task text", |event: &mut UsageEvent| {
                event.task_id = "fix the login bug in auth".to_string()
            }),
            ("task_id is an absolute path", |event: &mut UsageEvent| {
                event.task_id = "/srv/agent-worktrees/wt-1".to_string()
            }),
            ("skill_name carries free text", |event: &mut UsageEvent| {
                event.skill_name = "xavier memory\nignore all rules".to_string()
            }),
            (
                "package_hash exceeds the length bound",
                |event: &mut UsageEvent| event.package_hash = Some("a".repeat(MAX_FIELD_LEN + 1)),
            ),
        ];
        for (case, pollute) in cases {
            let mut event = event("evt-rejected", SkillStage::Selected);
            pollute(&mut event);
            let outcome = writer.record(&event, false);
            assert!(
                outcome.is_err(),
                "case '{}' must be rejected instead of stored",
                case
            );
        }
        assert_eq!(
            writer.count().expect("count must succeed"),
            0,
            "no rejected event may reach the table"
        );
    }

    #[test]
    fn the_schema_rejects_an_unknown_stage() {
        let conn = migrated();
        writer(&conn);
        let rejected = conn.execute(
            "INSERT INTO skill_usage_events (
                 event_id, workspace_id, task_id, tool, skill_name, stage, observed_at
             ) VALUES ('evt-x', 'ws', 'task', 'tool', 'skill', 'exfiltrated', ?1)",
            [NOW],
        );
        assert!(
            rejected.is_err(),
            "the stage CHECK must reject a stage outside the vocabulary"
        );
    }

    #[test]
    fn ensure_schema_prepares_a_database_without_the_migration() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        let writer = TelemetryWriter::new(&conn);
        writer
            .ensure_schema()
            .expect("the v6 DDL must be replayed on demand");
        let event = event("evt-repair", SkillStage::Selected);
        writer.record(&event, false).expect("record must succeed");
        assert_eq!(writer.count().expect("count must succeed"), 1);
    }

    #[test]
    fn concurrent_writers_persist_every_event() {
        const WORKERS: usize = 4;
        const PER_WORKER: usize = 25;

        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("workspace.db");
        {
            let conn = Connection::open(&db_path).expect("open workspace fixture");
            crate::storage::pragma::apply_pragmas(&conn).expect("apply pragmas");
            crate::storage::migrations::run(&conn).expect("baseline migrations must apply");
        }

        let handles: Vec<_> = (0..WORKERS)
            .map(|worker| {
                let path = db_path.clone();
                std::thread::spawn(move || insert_batch(&path, worker, PER_WORKER))
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .expect("worker thread must not panic")
                .expect("worker inserts must succeed");
        }

        let conn = Connection::open(&db_path).expect("reopen workspace fixture");
        assert_eq!(
            writer(&conn).count().expect("count must succeed"),
            (WORKERS * PER_WORKER) as u64,
            "every concurrent insert must be persisted exactly once"
        );
        for worker in 0..WORKERS {
            for index in 0..PER_WORKER {
                let event_id = format!("evt-w{worker}-{index}");
                let (stage, ..) = stored_row(&conn, &event_id)
                    .unwrap_or_else(|| panic!("concurrent event {event_id} must be readable"));
                assert_eq!(stage, "invoked", "event {event_id} must keep its stage");
            }
        }
    }

    /// Insert `PER_WORKER` events over its own connection, as one harness would.
    fn insert_batch(
        db_path: &Path,
        worker: usize,
        per_worker: usize,
    ) -> Result<(), TelemetryError> {
        let conn = Connection::open(db_path)?;
        crate::storage::pragma::apply_acquire_pragmas(&conn)?;
        let writer = TelemetryWriter::new(&conn);
        writer.ensure_schema()?;
        for index in 0..per_worker {
            let event = event(&format!("evt-w{worker}-{index}"), SkillStage::Invoked);
            writer.record(&event, false)?;
        }
        Ok(())
    }
}
