//! HumanChallenge Local SQLite Store
//!
//! Handles SQLite persistence for HumanChallenge events on the local node.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Result as SqliteResult};
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;

use crate::humanchallenge::types::{
    ChallengeStatus, ChallengeType, CurationVote, FarmingSummary, HumanChallengeEvent,
    IntrospectionSession,
};

const INIT_SQL: &str = "
    CREATE TABLE IF NOT EXISTS human_challenge_events (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        challenge_type TEXT NOT NULL,
        description TEXT NOT NULL,
        raw_content TEXT NOT NULL,
        confidence_score REAL NOT NULL,
        status TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        answered_at INTEGER,
        response TEXT,
        points_awarded INTEGER NOT NULL DEFAULT 0,
        privacy_p4_local_only INTEGER NOT NULL DEFAULT 1
    );
    CREATE INDEX IF NOT EXISTS idx_hc_session ON human_challenge_events(session_id);
    CREATE INDEX IF NOT EXISTS idx_hc_type ON human_challenge_events(challenge_type);
    CREATE INDEX IF NOT EXISTS idx_hc_status ON human_challenge_events(status);
    CREATE INDEX IF NOT EXISTS idx_hc_created ON human_challenge_events(created_at);
";

const VOTE_COLUMNS: &str = "id, challenge_id, verdict, curated_content, fact_verified, domain_tags, training_eligible, voted_at, technique";

/// Votes that may feed the training gate.
const GATE_VOTE_FILTER: &str = "training_eligible = 1 AND verdict IN ('accept', 'refine') AND NOT (technique IS NOT NULL AND fact_verified = 0)";

const MIGRATION_SQL: &str = "
    CREATE TABLE IF NOT EXISTS curation_votes (
        id TEXT PRIMARY KEY,
        challenge_id TEXT NOT NULL,
        verdict TEXT NOT NULL,
        curated_content TEXT,
        fact_verified INTEGER NOT NULL DEFAULT 0,
        domain_tags TEXT NOT NULL DEFAULT '[]',
        training_eligible INTEGER NOT NULL DEFAULT 0,
        voted_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_cv_challenge ON curation_votes(challenge_id);
    CREATE INDEX IF NOT EXISTS idx_cv_verdict ON curation_votes(verdict);
    CREATE INDEX IF NOT EXISTS idx_cv_training ON curation_votes(training_eligible);

    CREATE TABLE IF NOT EXISTS introspection_sessions (
        id TEXT PRIMARY KEY,
        challenge_id TEXT NOT NULL,
        technique TEXT NOT NULL,
        turns TEXT NOT NULL DEFAULT '[]',
        depth_score REAL NOT NULL DEFAULT 0.0,
        insights TEXT NOT NULL DEFAULT '[]',
        status TEXT NOT NULL DEFAULT 'active',
        started_at INTEGER NOT NULL,
        completed_at INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_is_challenge ON introspection_sessions(challenge_id);
    CREATE INDEX IF NOT EXISTS idx_is_status ON introspection_sessions(status);

    CREATE TABLE IF NOT EXISTS training_gate_log (
        id TEXT PRIMARY KEY,
        bundle_id TEXT NOT NULL,
        challenge_ids TEXT NOT NULL,
        record_count INTEGER NOT NULL,
        domain_tags TEXT NOT NULL DEFAULT '[]',
        privacy_level TEXT NOT NULL DEFAULT 'p4_local',
        triggered_at INTEGER NOT NULL,
        status TEXT NOT NULL DEFAULT 'pending'
    );
";

/// Why `verify_introspection_vote` could not apply a verdict.
#[derive(Debug)]
pub enum VerifyVoteError {
    /// No vote with that id.
    NotFound,
    /// The vote is not an introspection insight (no `technique`).
    NotIntrospection,
    /// The vote was already reviewed (verdict is no longer the pending `refine`).
    AlreadyReviewed,
    Db(rusqlite::Error),
}

impl std::fmt::Display for VerifyVoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "vote not found"),
            Self::NotIntrospection => write!(f, "vote is not an introspection insight"),
            Self::AlreadyReviewed => write!(f, "vote was already reviewed"),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VerifyVoteError {}

impl From<rusqlite::Error> for VerifyVoteError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

/// How the process-wide HumanChallenge/introspection store is backed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreBacking {
    /// Not reported yet (e.g. the daemon has not built the store).
    Unknown,
    File,
    Memory,
}

static STORE_BACKING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Record how the daemon's store is backed; read by `/health`.
pub fn set_store_backing(backing: StoreBacking) {
    let v = match backing {
        StoreBacking::Unknown => 0,
        StoreBacking::File => 1,
        StoreBacking::Memory => 2,
    };
    STORE_BACKING.store(v, std::sync::atomic::Ordering::Relaxed);
}

pub fn store_backing() -> StoreBacking {
    match STORE_BACKING.load(std::sync::atomic::Ordering::Relaxed) {
        1 => StoreBacking::File,
        2 => StoreBacking::Memory,
        _ => StoreBacking::Unknown,
    }
}

/// Reason string added to `/health` degraded reasons when the store is in-memory.
pub const MEMORY_FALLBACK_REASON: &str =
    "subsystem:humanchallenge_store: in-memory fallback active, challenges and introspection are not persisted";

/// Add `humanchallenge_store` (and, on memory fallback, a degraded reason) to a
/// serialized `/health` body.
pub fn annotate_health(health: &mut serde_json::Value, backing: StoreBacking) {
    let Some(obj) = health.as_object_mut() else {
        return;
    };
    let label = match backing {
        StoreBacking::Unknown => "unknown",
        StoreBacking::File => "file",
        StoreBacking::Memory => "memory",
    };
    obj.insert("humanchallenge_store".into(), label.into());
    if backing == StoreBacking::Memory {
        let reasons = obj
            .entry("degraded_reasons")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if let Some(list) = reasons.as_array_mut() {
            list.push(MEMORY_FALLBACK_REASON.into());
        }
        if obj.get("status").and_then(|s| s.as_str()) == Some("healthy") {
            obj.insert("status".into(), "degraded".into());
        }
    }
}

pub struct HumanChallengeStore {
    conn: Mutex<Connection>,
}

impl HumanChallengeStore {
    /// Initialize store at path
    pub fn new<P: AsRef<Path>>(db_path: P) -> SqliteResult<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(INIT_SQL)?;
        conn.execute_batch(MIGRATION_SQL)?;
        Self::migrate_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Additive column migrations for databases created by older versions.
    fn migrate_columns(conn: &Connection) -> SqliteResult<()> {
        let has_consent = {
            let mut stmt = conn.prepare("PRAGMA table_info(introspection_sessions)")?;
            let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for n in names {
                if n? == "training_consent" {
                    found = true;
                }
            }
            found
        };
        if !has_consent {
            conn.execute_batch(
                "ALTER TABLE introspection_sessions ADD COLUMN training_consent INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        let has_technique = {
            let mut stmt = conn.prepare("PRAGMA table_info(curation_votes)")?;
            let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut found = false;
            for n in names {
                if n? == "technique" {
                    found = true;
                }
            }
            found
        };
        if !has_technique {
            conn.execute_batch("ALTER TABLE curation_votes ADD COLUMN technique TEXT;")?;
        }
        Ok(())
    }

    /// Initialize an in-memory store for testing
    pub fn in_memory() -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(INIT_SQL)?;
        conn.execute_batch(MIGRATION_SQL)?;
        Self::migrate_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Save or ignore a HumanChallenge event
    pub fn save_event(&self, event: &HumanChallengeEvent) -> SqliteResult<()> {
        let created_ts = event.created_at.timestamp();
        let answered_ts = event.answered_at.map(|t| t.timestamp());
        let local_only = if event.privacy_p4_local_only { 1 } else { 0 };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO human_challenge_events
             (id, session_id, challenge_type, description, raw_content, confidence_score, status, created_at, answered_at, response, points_awarded, privacy_p4_local_only)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                event.id,
                event.session_id,
                event.challenge_type.as_str(),
                event.description,
                event.raw_content,
                event.confidence_score,
                event.status.as_str(),
                created_ts,
                answered_ts,
                event.response,
                event.points_awarded,
                local_only
            ],
        )?;

        Ok(())
    }

    /// Retrieve an event by ID
    pub fn get_event_by_id(&self, id: &str) -> SqliteResult<Option<HumanChallengeEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, challenge_type, description, raw_content, confidence_score, status, created_at, answered_at, response, points_awarded, privacy_p4_local_only
             FROM human_challenge_events WHERE id = ?1",
        )?;

        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::row_to_event(row)?))
        } else {
            Ok(None)
        }
    }

    /// List events filtered by optional status
    pub fn list_events(
        &self,
        status_filter: Option<ChallengeStatus>,
        limit: u32,
    ) -> SqliteResult<Vec<HumanChallengeEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut results = Vec::new();

        if let Some(status) = status_filter {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, challenge_type, description, raw_content, confidence_score, status, created_at, answered_at, response, points_awarded, privacy_p4_local_only
                 FROM human_challenge_events WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![status.as_str(), limit], Self::row_to_event)?;
            for row in rows {
                results.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, session_id, challenge_type, description, raw_content, confidence_score, status, created_at, answered_at, response, points_awarded, privacy_p4_local_only
                 FROM human_challenge_events ORDER BY created_at DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], Self::row_to_event)?;
            for row in rows {
                results.push(row?);
            }
        }

        Ok(results)
    }

    /// List events filtered by year_month ("YYYY-MM")
    pub fn list_events_by_month(
        &self,
        year_month: &str,
        limit: u32,
    ) -> SqliteResult<Vec<HumanChallengeEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, challenge_type, description, raw_content, confidence_score, status, created_at, answered_at, response, points_awarded, privacy_p4_local_only
             FROM human_challenge_events
             WHERE strftime('%Y-%m', datetime(created_at, 'unixepoch')) = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![year_month, limit], Self::row_to_event)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Record human response and award points
    pub fn answer_challenge(&self, id: &str, response: &str, points: u32) -> SqliteResult<bool> {
        let answered_ts = Utc::now().timestamp();
        let conn = self.conn.lock().unwrap();
        let rows_updated = conn.execute(
            "UPDATE human_challenge_events
             SET response = ?1, status = 'answered', answered_at = ?2, points_awarded = ?3
             WHERE id = ?4 AND status = 'candidate'",
            params![response, answered_ts, points, id],
        )?;

        Ok(rows_updated > 0)
    }

    /// Monthly X2 farming metrics summary calculation
    pub fn get_farming_summary(&self, year_month: &str) -> SqliteResult<FarmingSummary> {
        let conn = self.conn.lock().unwrap();
        // year_month format expected: "YYYY-MM"
        let mut stmt = conn.prepare(
            "SELECT
                COALESCE(SUM(points_awarded), 0) as total_pts,
                COUNT(CASE WHEN status IN ('answered', 'verified') THEN 1 END) as answered_cnt,
                COUNT(CASE WHEN status = 'verified' THEN 1 END) as verified_cnt
             FROM human_challenge_events
             WHERE strftime('%Y-%m', datetime(created_at, 'unixepoch')) = ?1",
        )?;

        let mut rows = stmt.query([year_month])?;
        if let Some(row) = rows.next()? {
            let total_pts: u32 = row.get(0)?;
            let answered_cnt: u32 = row.get(1)?;
            let verified_cnt: u32 = row.get(2)?;

            Ok(FarmingSummary {
                year_month: year_month.to_string(),
                total_points: total_pts,
                target_points: 10,
                answered_count: answered_cnt,
                verified_count: verified_cnt,
            })
        } else {
            Ok(FarmingSummary {
                year_month: year_month.to_string(),
                ..Default::default()
            })
        }
    }
    // -----------------------------------------------------------------------
    // Curation Vote methods
    // -----------------------------------------------------------------------

    /// Save a curation vote for a challenge event.
    pub fn save_curation_vote(&self, vote: &CurationVote) -> SqliteResult<()> {
        use crate::humanchallenge::types::CurationVote;
        let voted_ts = vote.voted_at.timestamp();
        let domain_tags_json =
            serde_json::to_string(&vote.domain_tags).unwrap_or_else(|_| "[]".to_string());
        let fact_v = if vote.fact_verified { 1 } else { 0 };
        let training_e = if vote.training_eligible { 1 } else { 0 };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO curation_votes
             (id, challenge_id, verdict, curated_content, fact_verified, domain_tags, training_eligible, voted_at, technique)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                vote.id,
                vote.challenge_id,
                vote.verdict.as_str(),
                vote.curated_content,
                fact_v,
                domain_tags_json,
                training_e,
                voted_ts,
                vote.technique
            ],
        )?;
        Ok(())
    }

    /// Shared decoder for `VOTE_COLUMNS` rows; unknown verdicts decode as `Refine`.
    fn row_to_vote(row: &rusqlite::Row) -> SqliteResult<CurationVote> {
        use crate::humanchallenge::types::CurationVerdict;

        let domain_tags_json: String = row.get(5)?;
        let voted_ts: i64 = row.get(7)?;
        let verdict_str: String = row.get(2)?;
        Ok(CurationVote {
            id: row.get(0)?,
            challenge_id: row.get(1)?,
            verdict: CurationVerdict::from_str(&verdict_str).unwrap_or(CurationVerdict::Refine),
            curated_content: row.get(3)?,
            fact_verified: row.get::<_, i32>(4)? != 0,
            domain_tags: serde_json::from_str(&domain_tags_json).unwrap_or_default(),
            training_eligible: row.get::<_, i32>(6)? != 0,
            voted_at: DateTime::from_timestamp(voted_ts, 0).unwrap_or_else(Utc::now),
            technique: row.get(8)?,
        })
    }

    /// Get curation votes eligible for training (accepted or refined + training_eligible).
    /// Unverified introspection votes (technique set, fact_verified=0) are pending a human
    /// check and never count toward the gate.
    pub fn get_training_eligible_votes(&self, limit: u32) -> SqliteResult<Vec<CurationVote>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {VOTE_COLUMNS}
             FROM curation_votes
             WHERE {GATE_VOTE_FILTER}
             ORDER BY voted_at DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_map(rusqlite::params![limit], Self::row_to_vote)?;
        rows.collect()
    }

    /// All curation votes for a challenge id (eligible or not), oldest first.
    pub fn get_votes_for_challenge(&self, challenge_id: &str) -> SqliteResult<Vec<CurationVote>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {VOTE_COLUMNS}
             FROM curation_votes WHERE challenge_id = ?1 ORDER BY voted_at ASC, id ASC"
        ))?;
        let rows = stmt.query_map(rusqlite::params![challenge_id], Self::row_to_vote)?;
        rows.collect()
    }

    /// Count accepted/refined training-eligible votes
    pub fn count_training_eligible(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM curation_votes WHERE {GATE_VOTE_FILTER}"),
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Introspection insights awaiting human verification (unreviewed `refine` votes
    /// carrying a technique), oldest first.
    pub fn list_pending_introspection_votes(&self, limit: u32) -> SqliteResult<Vec<CurationVote>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(&format!(
            "SELECT {VOTE_COLUMNS}
             FROM curation_votes
             WHERE technique IS NOT NULL AND verdict = 'refine' AND fact_verified = 0
             ORDER BY voted_at ASC, id ASC LIMIT ?1"
        ))?;
        let rows = stmt.query_map(rusqlite::params![limit], Self::row_to_vote)?;
        rows.collect()
    }

    /// Apply a human verdict to a pending introspection vote.
    ///
    /// `accept` with `fact_verified` makes the vote training-eligible only if training
    /// consent was recorded at completion (the vote's existing `training_eligible` flag);
    /// verification never grants consent. `reject` is never eligible. Votes that are not
    /// introspection insights are never touched.
    pub fn verify_introspection_vote(
        &self,
        id: &str,
        accept: bool,
        fact_verified: bool,
    ) -> Result<CurationVote, VerifyVoteError> {
        let conn = self.conn.lock().unwrap();
        let vote = {
            let mut stmt = conn.prepare(&format!(
                "SELECT {VOTE_COLUMNS} FROM curation_votes WHERE id = ?1"
            ))?;
            let mut rows = stmt.query([id])?;
            match rows.next()? {
                Some(row) => Self::row_to_vote(row)?,
                None => return Err(VerifyVoteError::NotFound),
            }
        };
        if vote.technique.is_none() {
            return Err(VerifyVoteError::NotIntrospection);
        }
        if vote.verdict != crate::humanchallenge::types::CurationVerdict::Refine
            || vote.fact_verified
        {
            return Err(VerifyVoteError::AlreadyReviewed);
        }
        let (verdict, fact_v, eligible) = if accept {
            (
                "accept",
                fact_verified,
                fact_verified && vote.training_eligible,
            )
        } else {
            ("reject", false, false)
        };
        conn.execute(
            "UPDATE curation_votes SET verdict = ?1, fact_verified = ?2, training_eligible = ?3
             WHERE id = ?4 AND technique IS NOT NULL",
            params![verdict, fact_v as i32, eligible as i32, id],
        )?;
        let mut updated = vote;
        updated.verdict = verdict.parse().unwrap_or(updated.verdict);
        updated.fact_verified = fact_v;
        updated.training_eligible = eligible;
        Ok(updated)
    }

    // -----------------------------------------------------------------------
    // Introspection Session methods
    // -----------------------------------------------------------------------

    /// Save or update an introspection session.
    pub fn save_introspection_session(&self, session: &IntrospectionSession) -> SqliteResult<()> {
        use crate::humanchallenge::types::IntrospectionSession;
        let turns_json = serde_json::to_string(&session.turns).unwrap_or_else(|_| "[]".to_string());
        let insights_json =
            serde_json::to_string(&session.insights).unwrap_or_else(|_| "[]".to_string());
        let started_ts = session.started_at.timestamp();
        let completed_ts = session.completed_at.map(|t| t.timestamp());

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO introspection_sessions
             (id, challenge_id, technique, turns, depth_score, insights, status, started_at, completed_at, training_consent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                session.id,
                session.challenge_id,
                session.technique.as_str(),
                turns_json,
                session.depth_score,
                insights_json,
                session.status.as_str(),
                started_ts,
                completed_ts,
                if session.training_consent { 1 } else { 0 }
            ],
        )?;
        Ok(())
    }

    /// Retrieve an introspection session by ID.
    pub fn get_introspection_session(
        &self,
        id: &str,
    ) -> SqliteResult<Option<IntrospectionSession>> {
        use crate::humanchallenge::types::{
            IntrospectionSession, IntrospectionStatus, IntrospectionTechnique, IntrospectionTurn,
        };

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, challenge_id, technique, turns, depth_score, insights, status, started_at, completed_at, training_consent
             FROM introspection_sessions WHERE id = ?1",
        )?;

        let mut rows = stmt.query([id])?;
        if let Some(row) = rows.next()? {
            let turns_json: String = row.get(3)?;
            let insights_json: String = row.get(5)?;
            let started_ts: i64 = row.get(7)?;
            let completed_ts: Option<i64> = row.get(8)?;
            let technique_str: String = row.get(2)?;
            let technique = match technique_str.as_str() {
                "socratic_questioning" => IntrospectionTechnique::SocraticQuestioning,
                "five_whys" => IntrospectionTechnique::FiveWhys,
                "pre_mortem" => IntrospectionTechnique::PreMortem,
                "steel_manning" => IntrospectionTechnique::SteelManning,
                "first_principles" => IntrospectionTechnique::FirstPrinciples,
                _ => IntrospectionTechnique::PatternRecognition,
            };
            Ok(Some(IntrospectionSession {
                id: row.get(0)?,
                challenge_id: row.get(1)?,
                technique,
                turns: serde_json::from_str(&turns_json).unwrap_or_default(),
                depth_score: row.get(4)?,
                insights: serde_json::from_str(&insights_json).unwrap_or_default(),
                status: IntrospectionStatus::from_str(&row.get::<_, String>(6)?).unwrap(),
                started_at: DateTime::from_timestamp(started_ts, 0).unwrap_or_else(Utc::now),
                completed_at: completed_ts.and_then(|ts| DateTime::from_timestamp(ts, 0)),
                training_consent: row.get::<_, i32>(9)? != 0,
            }))
        } else {
            Ok(None)
        }
    }

    // -----------------------------------------------------------------------
    // Training Gate Log methods
    // -----------------------------------------------------------------------

    /// Log a training bundle export triggered from curated challenges.
    pub fn log_training_gate(
        &self,
        bundle_id: &str,
        challenge_ids: &[String],
        record_count: usize,
        domain_tags: &[String],
        privacy_level: &str,
    ) -> SqliteResult<String> {
        let log_id = format!("tgl_{}", ulid::Ulid::new());
        let triggered_ts = Utc::now().timestamp();
        let challenge_ids_json =
            serde_json::to_string(challenge_ids).unwrap_or_else(|_| "[]".to_string());
        let domain_tags_json =
            serde_json::to_string(domain_tags).unwrap_or_else(|_| "[]".to_string());

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO training_gate_log
             (id, bundle_id, challenge_ids, record_count, domain_tags, privacy_level, triggered_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending')",
            rusqlite::params![
                log_id,
                bundle_id,
                challenge_ids_json,
                record_count as i64,
                domain_tags_json,
                privacy_level,
                triggered_ts
            ],
        )?;
        Ok(log_id)
    }

    fn row_to_event(row: &rusqlite::Row) -> SqliteResult<HumanChallengeEvent> {
        let id: String = row.get(0)?;
        let session_id: String = row.get(1)?;
        let challenge_type_str: String = row.get(2)?;
        let description: String = row.get(3)?;
        let raw_content: String = row.get(4)?;
        let confidence_score: f32 = row.get(5)?;
        let status_str: String = row.get(6)?;
        let created_ts: i64 = row.get(7)?;
        let answered_ts: Option<i64> = row.get(8)?;
        let response: Option<String> = row.get(9)?;
        let points_awarded: u32 = row.get(10)?;
        let local_only_int: i32 = row.get(11)?;

        let challenge_type = match challenge_type_str.as_str() {
            "contradiction" => ChallengeType::Contradiction,
            "decision" => ChallengeType::Decision,
            "execution" => ChallengeType::Execution,
            "assumption" => ChallengeType::Assumption,
            "clarification" => ChallengeType::Clarification,
            _ => ChallengeType::Decision,
        };

        let status = ChallengeStatus::from_str(&status_str).unwrap_or(ChallengeStatus::Candidate);
        let created_at = DateTime::from_timestamp(created_ts, 0).unwrap_or_else(Utc::now);
        let answered_at = answered_ts.and_then(|ts| DateTime::from_timestamp(ts, 0));

        Ok(HumanChallengeEvent {
            id,
            session_id,
            challenge_type,
            description,
            raw_content,
            confidence_score,
            status,
            created_at,
            answered_at,
            response,
            points_awarded,
            privacy_p4_local_only: local_only_int != 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanchallenge::types::CurationVerdict;

    #[test]
    fn test_store_save_get_and_answer() {
        let store = HumanChallengeStore::in_memory().unwrap();
        let event = HumanChallengeEvent::new(
            "sess_01",
            ChallengeType::Decision,
            "Architecture decision",
            "Decidimos usar SQLite",
            0.95,
        );

        store.save_event(&event).unwrap();

        let fetched = store.get_event_by_id(&event.id).unwrap().unwrap();
        assert_eq!(fetched.id, event.id);
        assert_eq!(fetched.status, ChallengeStatus::Candidate);

        let answered = store.answer_challenge(&event.id, "Confirmado", 10).unwrap();
        assert!(answered);

        let updated = store.get_event_by_id(&event.id).unwrap().unwrap();
        assert_eq!(updated.status, ChallengeStatus::Answered);
        assert_eq!(updated.points_awarded, 10);
        assert_eq!(updated.response.as_deref(), Some("Confirmado"));
    }

    #[test]
    fn test_store_list_events_by_month() {
        let store = HumanChallengeStore::in_memory().unwrap();
        let event = HumanChallengeEvent::new(
            "sess_02",
            ChallengeType::Contradiction,
            "Contradiction test",
            "Sin embargo contradice",
            0.85,
        );
        store.save_event(&event).unwrap();

        let current_month = Utc::now().format("%Y-%m").to_string();
        let month_events = store.list_events_by_month(&current_month, 10).unwrap();
        assert_eq!(month_events.len(), 1);
        assert_eq!(month_events[0].id, event.id);
    }

    #[test]
    fn test_unknown_verdict_decodes_identically_in_both_readers() {
        let store = HumanChallengeStore::in_memory().unwrap();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO curation_votes (id, challenge_id, verdict, fact_verified, domain_tags, training_eligible, voted_at)
                 VALUES ('cv_odd', 'hc_odd', 'weird_verdict', 1, '[]', 1, 0)",
                [],
            )
            .unwrap();
        }
        let by_challenge = store.get_votes_for_challenge("hc_odd").unwrap();
        let eligible = store.get_training_eligible_votes(10).unwrap();
        // The eligible query only matches accept/refine, so compare the shared decoder directly.
        assert_eq!(by_challenge.len(), 1);
        assert_eq!(by_challenge[0].verdict, CurationVerdict::Refine);
        assert!(eligible.is_empty());
        store
            .conn
            .lock()
            .unwrap()
            .execute("UPDATE curation_votes SET verdict = 'refine'", [])
            .unwrap();
        let eligible = store.get_training_eligible_votes(10).unwrap();
        assert_eq!(eligible[0].verdict, by_challenge[0].verdict);
    }

    #[test]
    fn test_annotate_health_reports_backing_and_memory_reason() {
        let mut file = serde_json::json!({"status": "healthy", "degraded_reasons": []});
        annotate_health(&mut file, StoreBacking::File);
        assert_eq!(file["humanchallenge_store"], "file");
        assert_eq!(file["status"], "healthy");
        assert_eq!(file["degraded_reasons"].as_array().unwrap().len(), 0);

        let mut mem = serde_json::json!({"status": "healthy", "degraded_reasons": []});
        annotate_health(&mut mem, StoreBacking::Memory);
        assert_eq!(mem["humanchallenge_store"], "memory");
        assert_eq!(mem["status"], "degraded");
        assert_eq!(mem["degraded_reasons"][0], MEMORY_FALLBACK_REASON);

        let mut unknown = serde_json::json!({"status": "healthy"});
        annotate_health(&mut unknown, StoreBacking::Unknown);
        assert_eq!(unknown["humanchallenge_store"], "unknown");
    }

    #[test]
    fn test_store_backing_global_roundtrip() {
        set_store_backing(StoreBacking::Memory);
        assert_eq!(store_backing(), StoreBacking::Memory);
        set_store_backing(StoreBacking::File);
        assert_eq!(store_backing(), StoreBacking::File);
        set_store_backing(StoreBacking::Unknown);
    }
}
