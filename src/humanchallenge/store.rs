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

pub struct HumanChallengeStore {
    conn: Mutex<Connection>,
}

impl HumanChallengeStore {
    /// Initialize store at path
    pub fn new<P: AsRef<Path>>(db_path: P) -> SqliteResult<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(INIT_SQL)?;
        conn.execute_batch(MIGRATION_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Initialize an in-memory store for testing
    pub fn in_memory() -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(INIT_SQL)?;
        conn.execute_batch(MIGRATION_SQL)?;
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
        let domain_tags_json = serde_json::to_string(&vote.domain_tags)
            .unwrap_or_else(|_| "[]".to_string());
        let fact_v = if vote.fact_verified { 1 } else { 0 };
        let training_e = if vote.training_eligible { 1 } else { 0 };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO curation_votes
             (id, challenge_id, verdict, curated_content, fact_verified, domain_tags, training_eligible, voted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                vote.id,
                vote.challenge_id,
                vote.verdict.as_str(),
                vote.curated_content,
                fact_v,
                domain_tags_json,
                training_e,
                voted_ts
            ],
        )?;
        Ok(())
    }

    /// Get all curation votes eligible for training (accepted or refined + training_eligible).
    pub fn get_training_eligible_votes(&self, limit: u32) -> SqliteResult<Vec<CurationVote>> {
        use crate::humanchallenge::types::{CurationVote, CurationVerdict};
        use std::str::FromStr;

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, challenge_id, verdict, curated_content, fact_verified, domain_tags, training_eligible, voted_at
             FROM curation_votes
             WHERE training_eligible = 1 AND verdict IN ('accept', 'refine')
             ORDER BY voted_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(rusqlite::params![limit], |row| {
            let domain_tags_json: String = row.get(5)?;
            let domain_tags: Vec<String> =
                serde_json::from_str(&domain_tags_json).unwrap_or_default();
            let voted_ts: i64 = row.get(7)?;
            Ok(CurationVote {
                id: row.get(0)?,
                challenge_id: row.get(1)?,
                verdict: CurationVerdict::from_str(&row.get::<_, String>(2)?).unwrap(),
                curated_content: row.get(3)?,
                fact_verified: row.get::<_, i32>(4)? != 0,
                domain_tags,
                training_eligible: row.get::<_, i32>(6)? != 0,
                voted_at: DateTime::from_timestamp(voted_ts, 0).unwrap_or_else(Utc::now),
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Count accepted/refined training-eligible votes
    pub fn count_training_eligible(&self) -> SqliteResult<usize> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM curation_votes WHERE training_eligible = 1 AND verdict IN ('accept', 'refine')",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    // -----------------------------------------------------------------------
    // Introspection Session methods
    // -----------------------------------------------------------------------

    /// Save or update an introspection session.
    pub fn save_introspection_session(&self, session: &IntrospectionSession) -> SqliteResult<()> {
        use crate::humanchallenge::types::IntrospectionSession;
        let turns_json = serde_json::to_string(&session.turns)
            .unwrap_or_else(|_| "[]".to_string());
        let insights_json = serde_json::to_string(&session.insights)
            .unwrap_or_else(|_| "[]".to_string());
        let started_ts = session.started_at.timestamp();
        let completed_ts = session.completed_at.map(|t| t.timestamp());

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO introspection_sessions
             (id, challenge_id, technique, turns, depth_score, insights, status, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                session.id,
                session.challenge_id,
                session.technique.as_str(),
                turns_json,
                session.depth_score,
                insights_json,
                session.status.as_str(),
                started_ts,
                completed_ts
            ],
        )?;
        Ok(())
    }

    /// Retrieve an introspection session by ID.
    pub fn get_introspection_session(&self, id: &str) -> SqliteResult<Option<IntrospectionSession>> {
        use crate::humanchallenge::types::{
            IntrospectionSession, IntrospectionStatus, IntrospectionTechnique, IntrospectionTurn,
        };
        use std::str::FromStr;

        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, challenge_id, technique, turns, depth_score, insights, status, started_at, completed_at
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
        let challenge_ids_json = serde_json::to_string(challenge_ids)
            .unwrap_or_else(|_| "[]".to_string());
        let domain_tags_json = serde_json::to_string(domain_tags)
            .unwrap_or_else(|_| "[]".to_string());

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
}
