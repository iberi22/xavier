use crate::consent::ConsentState;
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
    pub fn record(
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
    pub fn aggregate(&self, state: ConsentState) -> Result<Vec<AggregateRow>, TelemetryError> {
        if state == ConsentState::Off {
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
        // serde_json cannot hold a non-finite number: "1e500" fails to parse and
        // json!(f64::INFINITY) becomes Null, so the value must never survive as
        // a non-finite f64.
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
        let store = TelemetryStore::open_in_memory().unwrap();
        assert!(!store
            .record(
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
        let store = TelemetryStore::open_in_memory().unwrap();
        let mut fields = Map::new();
        fields.insert("metric_name".into(), json!("m"));
        assert!(store
            .record(
                ConsentState::Local,
                &Sample {
                    kind: "k".into(),
                    fields
                },
                1
            )
            .unwrap());
        let aggs = store.aggregate(ConsentState::Local).unwrap();
        assert_eq!(aggs[0].count, 1);
    }
    #[test]
    fn test_aggregate_off() {
        let store = TelemetryStore::open_in_memory().unwrap();
        assert!(matches!(
            store.aggregate(ConsentState::Off),
            Err(TelemetryError::NotOptedIn)
        ));
    }
    #[test]
    fn test_aggregate_shared() {
        let store = TelemetryStore::open_in_memory().unwrap();
        store
            .record(
                ConsentState::Shared,
                &Sample {
                    kind: "k".into(),
                    fields: Map::new(),
                },
                0,
            )
            .unwrap();
        let aggs = store.aggregate(ConsentState::Shared).unwrap();
        assert_eq!(aggs[0].count, 1);
    }
    #[test]
    fn test_stored_row_only_internal() {
        let store = TelemetryStore::open_in_memory().unwrap();
        let mut fields = Map::new();
        fields.insert("metric_name".into(), json!("m"));
        fields.insert("unknown".into(), json!("x"));
        store
            .record(
                ConsentState::Local,
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
