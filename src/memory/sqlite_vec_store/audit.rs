use anyhow::{Result};
use libsql::{params, Connection};
use sha2::{Digest, Sha256};
use crate::memory::store::MemoryRecord;
use crate::server::events::RealtimeEvent;

use super::VecSqliteMemoryStore;

impl VecSqliteMemoryStore {
    pub(crate) async fn append_timeline_event(
        &self,
        conn: &Connection,
        workspace_id: &str,
        record: &MemoryRecord,
    ) -> Result<()> {
        if !Self::audit_chain_enabled() {
            return Ok(());
        }

        let mut seq_stmt = conn.prepare("SELECT COALESCE(MAX(sequence), 0) + 1 FROM timeline_events WHERE workspace_id = ?").await?;
        let mut seq_rows = seq_stmt.query(params![workspace_id]).await?;
        let next_sequence: i64 = if let Some(row) = seq_rows.next().await? {
            row.get::<i64>(0).unwrap_or(1)
        } else {
            1
        };

        let mut hash_stmt = conn.prepare("SELECT id, curr_hash FROM timeline_events WHERE workspace_id = ? ORDER BY sequence DESC, id DESC LIMIT 1").await?;
        let mut hash_rows = hash_stmt.query(params![workspace_id]).await?;
        let (previous_event_id, previous_hash): (Option<String>, Option<String>) = if let Some(row) = hash_rows.next().await? {
            (row.get::<Option<String>>(0).ok().flatten(), row.get::<Option<String>>(1).ok().flatten())
        } else {
            (None, None)
        };

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

        let operation = if record.revision > 1 { "update" } else { "create" };
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
                operation.clone(),
                format!("{} {}", operation, record.path),
                serde_json::to_string(&record.metadata).unwrap_or_default(),
                agent_id.clone(),
                previous_event_id,
                curr_hash
            ],
        ).await?;

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
        let conn = self.pool.get().await?;
        let mut stmt = conn.prepare(
            "SELECT id, memory_id, sequence, timestamp, operation, summary, details, agent_id FROM timeline_events 
             WHERE workspace_id = ? AND timestamp > ? ORDER BY sequence ASC",
        ).await?;

        let mut rows = stmt.query(params![workspace_id, since]).await?;
        let mut events = Vec::new();
        while let Some(row) = rows.next().await? {
            let event_id: String = row.get(0).map_err(anyhow::Error::msg)?;
            let agent_id: String = row.get(7).map_err(anyhow::Error::msg)?;
            let timestamp: String = row.get(3).map_err(anyhow::Error::msg)?;
            events.push(RealtimeEvent {
                workspace_id: workspace_id.to_string(),
                event_id,
                agent_id,
                project_id: None,
                event_type: "timeline_event".to_string(),
                timestamp,
                payload: serde_json::json!({
                    "id": row.get::<String>(0).map_err(anyhow::Error::msg)?,
                    "memory_id": row.get::<String>(1).map_err(anyhow::Error::msg)?,
                    "sequence": row.get::<i64>(2).map_err(anyhow::Error::msg)?,
                    "timestamp": row.get::<String>(3).map_err(anyhow::Error::msg)?,
                    "operation": row.get::<String>(4).map_err(anyhow::Error::msg)?,
                    "summary": row.get::<String>(5).ok(),
                    "details": serde_json::from_str::<serde_json::Value>(&row.get::<String>(6).unwrap_or_default()).unwrap_or_default(),
                    "agent_id": row.get::<String>(7).map_err(anyhow::Error::msg)?,
                }),
            });
        }
        Ok(events)
    }
}
