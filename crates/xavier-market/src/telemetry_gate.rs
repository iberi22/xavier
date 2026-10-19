use crate::consent::{ConsentError, ConsentRecord, ConsentState, ConsentStore};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use thiserror::Error;

pub const INTERNAL_FIELDS: [&str; 4] = ["metric_name", "value", "timestamp", "event_kind"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub kind: String,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sanitized {
    pub kind: String,
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AggregateRow {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("Not opted in")]
    NotOptedIn,
    #[error("Consent error: {0}")]
    Consent(#[from] ConsentError),
    #[error("DB err: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("JSON err: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn sanitize(sample: &Sample) -> Sanitized {
    let mut fields = Map::new();
    for key in INTERNAL_FIELDS.iter() {
        if let Some(val) = sample.fields.get(*key) {
            if let Value::Number(num) = val {
                if num.is_f64() && !num.as_f64().unwrap().is_finite() {
                    continue;
                }
            }
            fields.insert(key.to_string(), val.clone());
        }
    }
    Sanitized {
        kind: sample.kind.clone(),
        fields,
    }
}

pub struct TelemetryStore {
    conn: Mutex<Connection>,
}

impl TelemetryStore {
    pub fn open(path: &Path) -> Result<Self, TelemetryError> {
        Self::init(Connection::open(path)?)
    }
    pub fn open_in_memory() -> Result<Self, TelemetryError> {
        Self::init(Connection::open_in_memory()?)
    }
    fn init(conn: Connection) -> Result<Self, TelemetryError> {
        conn.execute_batch("CREATE TABLE IF NOT EXISTS market_telemetry (seq INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, payload_json TEXT NOT NULL, created_at_ms INTEGER NOT NULL);")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Records a telemetry sample by checking the node's consent state from `consent_store`.
    pub fn record(
        &self,
        consent_store: &ConsentStore,
        node_id: &str,
        sample: &Sample,
        now_ms: i64,
    ) -> Result<bool, TelemetryError> {
        let state = consent_store.state_of(node_id)?;
        self.record_raw(state, sample, now_ms)
    }

    /// Records a telemetry sample directly for a known `ConsentState`.
    pub fn record_raw(
        &self,
        state: ConsentState,
        sample: &Sample,
        now_ms: i64,
    ) -> Result<bool, TelemetryError> {
        if !state.may_collect() {
            return Ok(false);
        }
        let s = sanitize(sample);
        let pj = serde_json::to_string(&s.fields)?;
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        conn.execute(
            "INSERT INTO market_telemetry (kind, payload_json, created_at_ms) VALUES (?1, ?2, ?3)",
            params![s.kind, pj, now_ms],
        )?;
        Ok(true)
    }

    /// Computes aggregate metrics across telemetry kinds for the given `node_id`.
    /// Enforces the reciprocity rule: reads consent from `consent_store` and requires
    /// `Shared` (or `SharedSensitive`) consent (`state.may_leave_node()`). Local or Off nodes
    /// are refused with `TelemetryError::NotOptedIn`.
    pub fn aggregate(
        &self,
        consent_store: &ConsentStore,
        node_id: &str,
    ) -> Result<Vec<AggregateRow>, TelemetryError> {
        let state = consent_store.state_of(node_id)?;
        self.aggregate_raw(state)
    }

    /// Computes aggregate metrics directly for a given `ConsentState`.
    /// Enforces the reciprocity rule: only states where telemetry is shared with the network
    /// may fetch aggregates.
    pub fn aggregate_raw(&self, state: ConsentState) -> Result<Vec<AggregateRow>, TelemetryError> {
        if !state.may_leave_node() {
            return Err(TelemetryError::NotOptedIn);
        }
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stmt = conn.prepare("SELECT kind, COUNT(*) FROM market_telemetry GROUP BY kind")?;
        let rows = stmt.query_map([], |row| {
            Ok(AggregateRow {
                kind: row.get(0)?,
                count: row.get(1)?,
            })
        })?;
        let mut aggs = Vec::new();
        for row in rows {
            aggs.push(row?);
        }
        Ok(aggs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn helper_consent_store() -> ConsentStore {
        let store = ConsentStore::open_in_memory().unwrap();
        store
            .set(ConsentRecord {
                node_id: "node_off".into(),
                state: ConsentState::Off,
                terms_version: "v1".into(),
                consent_version: "v1".into(),
                updated_at_ms: 100,
            })
            .unwrap();
        store
            .set(ConsentRecord {
                node_id: "node_local".into(),
                state: ConsentState::Local,
                terms_version: "v1".into(),
                consent_version: "v1".into(),
                updated_at_ms: 100,
            })
            .unwrap();
        store
            .set(ConsentRecord {
                node_id: "node_shared".into(),
                state: ConsentState::Shared,
                terms_version: "v1".into(),
                consent_version: "v1".into(),
                updated_at_ms: 100,
            })
            .unwrap();
        store
            .set(ConsentRecord {
                node_id: "node_shared_sens".into(),
                state: ConsentState::SharedSensitive,
                terms_version: "v1".into(),
                consent_version: "v1".into(),
                updated_at_ms: 100,
            })
            .unwrap();
        store
    }

    #[test]
    fn test_sanitize() {
        let mut fields = Map::new();
        fields.insert("value".into(), json!(123.45));
        fields.insert("unknown".into(), json!("drop"));
        let s = sanitize(&Sample {
            kind: "k".into(),
            fields,
        });
        assert!(s.fields.contains_key("value"));
        assert!(!s.fields.contains_key("unknown"));
    }

    #[test]
    fn test_sanitize_non_finite() {
        let mut fields = Map::new();
        fields.insert(
            "value".into(),
            serde_json::from_str("1e500").unwrap_or(json!(f64::INFINITY)),
        );
        let s = sanitize(&Sample {
            kind: "k".into(),
            fields,
        });
        assert!(s
            .fields
            .get("value")
            .and_then(Value::as_f64)
            .is_none_or(f64::is_finite));
    }

    #[test]
    fn test_record_off() {
        let consent = helper_consent_store();
        let store = TelemetryStore::open_in_memory().unwrap();
        assert!(!store
            .record(
                &consent,
                "node_off",
                &Sample {
                    kind: "k".into(),
                    fields: Map::new()
                },
                0
            )
            .unwrap());
        assert!(!store
            .record_raw(
                ConsentState::Off,
                &Sample {
                    kind: "k".into(),
                    fields: Map::new()
                },
                0
            )
            .unwrap());
    }

    #[test]
    fn test_record_local() {
        let consent = helper_consent_store();
        let store = TelemetryStore::open_in_memory().unwrap();
        let mut fields = Map::new();
        fields.insert("metric_name".into(), json!("m"));
        assert!(store
            .record(
                &consent,
                "node_local",
                &Sample {
                    kind: "k".into(),
                    fields
                },
                1
            )
            .unwrap());
    }

    #[test]
    fn test_aggregate_off_and_unknown_refused() {
        let consent = helper_consent_store();
        let store = TelemetryStore::open_in_memory().unwrap();
        assert!(matches!(
            store.aggregate(&consent, "node_off"),
            Err(TelemetryError::NotOptedIn)
        ));
        assert!(matches!(
            store.aggregate(&consent, "node_unknown"),
            Err(TelemetryError::NotOptedIn)
        ));
        assert!(matches!(
            store.aggregate_raw(ConsentState::Off),
            Err(TelemetryError::NotOptedIn)
        ));
    }

    #[test]
    fn test_aggregate_local_denied_by_reciprocity() {
        let consent = helper_consent_store();
        let store = TelemetryStore::open_in_memory().unwrap();
        let mut fields = Map::new();
        fields.insert("metric_name".into(), json!("m"));
        store
            .record(
                &consent,
                "node_local",
                &Sample {
                    kind: "k".into(),
                    fields,
                },
                1,
            )
            .unwrap();

        // Node local can record locally, but reciprocity denies reading mesh aggregates
        assert!(matches!(
            store.aggregate(&consent, "node_local"),
            Err(TelemetryError::NotOptedIn)
        ));
        assert!(matches!(
            store.aggregate_raw(ConsentState::Local),
            Err(TelemetryError::NotOptedIn)
        ));
    }

    #[test]
    fn test_aggregate_shared_and_shared_sensitive_permitted() {
        let consent = helper_consent_store();
        let store = TelemetryStore::open_in_memory().unwrap();
        store
            .record(
                &consent,
                "node_shared",
                &Sample {
                    kind: "k".into(),
                    fields: Map::new(),
                },
                0,
            )
            .unwrap();
        let aggs = store.aggregate(&consent, "node_shared").unwrap();
        assert_eq!(aggs[0].count, 1);

        let aggs_sens = store.aggregate(&consent, "node_shared_sens").unwrap();
        assert_eq!(aggs_sens[0].count, 1);

        let aggs_raw = store.aggregate_raw(ConsentState::Shared).unwrap();
        assert_eq!(aggs_raw[0].count, 1);
    }

    #[test]
    fn test_stored_row_only_internal() {
        let consent = helper_consent_store();
        let store = TelemetryStore::open_in_memory().unwrap();
        let mut fields = Map::new();
        fields.insert("metric_name".into(), json!("m"));
        fields.insert("unknown".into(), json!("x"));
        store
            .record(
                &consent,
                "node_local",
                &Sample {
                    kind: "k".into(),
                    fields,
                },
                1,
            )
            .unwrap();
        let conn = store.conn.lock().unwrap();
        let pj: String = conn
            .query_row("SELECT payload_json FROM market_telemetry", [], |row| {
                row.get(0)
            })
            .unwrap();
        let parsed: Map<String, Value> = serde_json::from_str(&pj).unwrap();
        assert!(parsed.contains_key("metric_name"));
        assert!(!parsed.contains_key("unknown"));
    }
}
