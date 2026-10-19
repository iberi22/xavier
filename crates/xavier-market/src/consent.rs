use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsentState {
    #[default]
    Off,
    Local,
    Shared,
    SharedSensitive,
}

impl ConsentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Local => "local",
            Self::Shared => "shared",
            Self::SharedSensitive => "shared_sensitive",
        }
    }

    pub fn parse(value: &str) -> Result<Self, ConsentError> {
        match value.trim() {
            "off" => Ok(Self::Off),
            "local" => Ok(Self::Local),
            "shared" => Ok(Self::Shared),
            "shared_sensitive" => Ok(Self::SharedSensitive),
            other => Err(ConsentError::UnknownState(other.to_string())),
        }
    }

    pub fn may_collect(self) -> bool {
        match self {
            Self::Off => false,
            Self::Local | Self::Shared | Self::SharedSensitive => true,
        }
    }

    pub fn may_leave_node(self) -> bool {
        match self {
            Self::Off | Self::Local => false,
            Self::Shared | Self::SharedSensitive => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsentRecord {
    pub node_id: String,
    pub state: ConsentState,
    pub terms_version: String,
    pub consent_version: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Error)]
pub enum ConsentError {
    #[error("Missing consent or terms version")]
    MissingVersion,
    #[error("Unknown state: {0}")]
    UnknownState(String),
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct ConsentStore {
    conn: Mutex<Connection>,
}

impl ConsentStore {
    pub fn open(path: &Path) -> Result<Self, ConsentError> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, ConsentError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, ConsentError> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS market_consent (
                node_id TEXT PRIMARY KEY,
                state TEXT NOT NULL,
                terms_version TEXT NOT NULL,
                consent_version TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
            );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn set(&self, record: ConsentRecord) -> Result<(), ConsentError> {
        if record.state != ConsentState::Off
            && (record.terms_version.trim().is_empty() || record.consent_version.trim().is_empty())
        {
            return Err(ConsentError::MissingVersion);
        }
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.execute(
            "INSERT INTO market_consent (node_id, state, terms_version, consent_version, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(node_id) DO UPDATE SET
                 state = excluded.state,
                 terms_version = excluded.terms_version,
                 consent_version = excluded.consent_version,
                 updated_at_ms = excluded.updated_at_ms",
            params![
                record.node_id,
                record.state.as_str(),
                record.terms_version,
                record.consent_version,
                record.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, node_id: &str) -> Result<Option<ConsentRecord>, ConsentError> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stmt = conn.prepare("SELECT node_id, state, terms_version, consent_version, updated_at_ms FROM market_consent WHERE node_id = ?1")?;
        let res = stmt
            .query_row(params![node_id], |row| {
                let node_id: String = row.get(0)?;
                let state_str: String = row.get(1)?;
                let terms_version: String = row.get(2)?;
                let consent_version: String = row.get(3)?;
                let updated_at_ms: i64 = row.get(4)?;
                Ok((
                    node_id,
                    state_str,
                    terms_version,
                    consent_version,
                    updated_at_ms,
                ))
            })
            .optional()?;

        match res {
            Some((nid, state_str, tv, cv, upd)) => {
                let state = ConsentState::parse(&state_str)?;
                Ok(Some(ConsentRecord {
                    node_id: nid,
                    state,
                    terms_version: tv,
                    consent_version: cv,
                    updated_at_ms: upd,
                }))
            }
            None => Ok(None),
        }
    }

    pub fn state_of(&self, node_id: &str) -> Result<ConsentState, ConsentError> {
        if let Some(record) = self.get(node_id)? {
            Ok(record.state)
        } else {
            Ok(ConsentState::Off)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_default_state() {
        let state = ConsentState::default();
        assert_eq!(state, ConsentState::Off);
        assert!(!state.may_collect());
        assert!(!state.may_leave_node());
    }

    #[test]
    fn test_local_collects_but_cannot_leave() {
        let state = ConsentState::Local;
        assert!(state.may_collect());
        assert!(!state.may_leave_node());
    }

    #[test]
    fn test_shared_may_leave() {
        assert!(ConsentState::Shared.may_leave_node());
        assert!(ConsentState::SharedSensitive.may_leave_node());
    }

    #[test]
    fn test_downgrade_to_off() {
        let store = ConsentStore::open_in_memory().unwrap();
        let mut rec = ConsentRecord {
            node_id: "node1".into(),
            state: ConsentState::Shared,
            terms_version: "v1".into(),
            consent_version: "v1".into(),
            updated_at_ms: 100,
        };
        store.set(rec.clone()).unwrap();
        assert!(store.state_of("node1").unwrap().may_leave_node());

        rec.state = ConsentState::Off;
        rec.updated_at_ms = 200;
        store.set(rec).unwrap();
        assert!(!store.state_of("node1").unwrap().may_leave_node());
    }

    #[test]
    fn test_opt_in_without_versions_refused() {
        let store = ConsentStore::open_in_memory().unwrap();
        let rec = ConsentRecord {
            node_id: "node1".into(),
            state: ConsentState::Shared,
            terms_version: "".into(),
            consent_version: "v1".into(),
            updated_at_ms: 100,
        };
        assert!(matches!(store.set(rec), Err(ConsentError::MissingVersion)));
    }

    #[test]
    fn test_state_survives_reopening_file() {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("market_test_{}", ts));
        fs::create_dir_all(&temp_dir).unwrap();
        let db_path = temp_dir.join("consent.db");
        {
            let store = ConsentStore::open(&db_path).unwrap();
            let rec = ConsentRecord {
                node_id: "node1".into(),
                state: ConsentState::Shared,
                terms_version: "v1".into(),
                consent_version: "v1".into(),
                updated_at_ms: 100,
            };
            store.set(rec).unwrap();
        }
        {
            let store2 = ConsentStore::open(&db_path).unwrap();
            assert_eq!(store2.state_of("node1").unwrap(), ConsentState::Shared);
        }
        let _ = fs::remove_dir_all(temp_dir);
    }
}
