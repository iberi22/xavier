//! Audit logging for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::store::MemoryRecord;
use crate::server::events::RealtimeEvent;
use anyhow::Result;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use super::VecSqliteMemoryStore;

impl VecSqliteMemoryStore {
    pub(crate) fn append_timeline_event(
        &self,
        conn: &Connection,
        workspace_id: &str,
        record: &MemoryRecord,
    ) -> Result<()> {
        if !Self::audit_chain_enabled() {
            return Ok(());
        }

        let next_sequence: i64 = conn.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM timeline_events WHERE workspace_id = ?",
                params![workspace_id],
                |row| row.get(0)
            )?;

        let (previous_event_id, previous_hash): (Option<String>, Option<String>) =
            conn.query_row(
                "SELECT id, curr_hash FROM timeline_events WHERE workspace_id = ? ORDER BY sequence DESC, id DESC LIMIT 1",
                params![workspace_id],
                |row| Ok((row.get(0)?, row.get(1)?))
            ).unwrap_or((None, None));

        let event_id = ulid::Ulid::new().to_string();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let agent_id = record
            .metadata
            .get("_audit")
            .and_then(|value| value.get("agent_id"))
            .and_then(|value| value.as_str())
            .or_else(|| {
                record
                    .metadata
                    .get("agent_id")
                    .and_then(|value| value.as_str())
            })
            .unwrap_or("unknown")
            .to_string();

        let operation = if record.revision > 1 {
            "update"
        } else {
            "create"
        };
        let content_hash = format!("{:x}", Sha256::digest(record.content.as_bytes()));
        let curr_hash = format!(
            "{:x}",
            Sha256::digest(
                format!(
                    "{}:{}:{}:{}:{}",
                    previous_hash.unwrap_or_default(),
                    event_id,
                    timestamp,
                    operation,
                    content_hash
                )
                .as_bytes()
            )
        );

        conn.execute(
            "INSERT INTO timeline_events (id, workspace_id, memory_id, sequence, timestamp, operation, summary, details, agent_id, prev_hash, curr_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                event_id.clone(),
                workspace_id.to_string(),
                record.id.clone(),
                next_sequence,
                timestamp.clone(),
                operation,
                format!("{} {}", operation, record.path),
                serde_json::to_string(&record.metadata).unwrap_or_default(),
                agent_id.clone(),
                previous_event_id,
                curr_hash
            ],
        )?;

        // Broadcast event if channel exists
        if let Some(tx) = self.event_tx_ref() {
            let _ = tx.send(RealtimeEvent {
                workspace_id: workspace_id.to_string(),
                event_id: event_id.clone(),
                agent_id: agent_id.clone(),
                project_id: None,
                event_type: "timeline_event".to_string(),
                timestamp: timestamp.clone(),
                payload: serde_json::json!({
                    "id": event_id,
                    "memory_id": record.id.clone(),
                    "operation": operation,
                    "timestamp": timestamp,
                    "path": record.path,
                }),
            });
        }

        Ok(())
    }

    pub(crate) async fn perform_list_timeline_events(
        &self,
        workspace_id: &str,
        since: &str,
    ) -> Result<Vec<RealtimeEvent>> {
        let workspace_id = workspace_id.to_string();
        let since = since.to_string();

        crate::codebase::connection_manager::ConnectionManager::global().with_conn(&self.project_id, move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, memory_id, sequence, timestamp, operation, summary, details, agent_id FROM timeline_events
                 WHERE workspace_id = ? AND timestamp > ? ORDER BY sequence ASC",
            )?;

            let mut rows = stmt.query(params![workspace_id, since])?;
            let mut events = Vec::new();
            while let Some(row) = rows.next()? {
                let event_id: String = row.get(0)?;
                let agent_id: String = row.get(7)?;
                let timestamp: String = row.get(3)?;
                let details_str: String = row.get(6)?;
                events.push(RealtimeEvent {
                    workspace_id: workspace_id.to_string(),
                    event_id: event_id.clone(),
                    agent_id,
                    project_id: None,
                    event_type: "timeline_event".to_string(),
                    timestamp,
                    payload: serde_json::json!({
                        "id": event_id,
                        "memory_id": row.get::<_, String>(1)?,
                        "sequence": row.get::<_, i64>(2)?,
                        "timestamp": row.get::<_, String>(3)?,
                        "operation": row.get::<_, String>(4)?,
                        "summary": row.get::<_, Option<String>>(5)?,
                        "details": serde_json::from_str::<serde_json::Value>(&details_str).unwrap_or_default(),
                        "agent_id": row.get::<_, String>(7)?,
                    }),
                });
            }
            Ok(events)
        }).await
    }
}
