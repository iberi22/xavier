// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Timeline Engine (Time Travel)
//!
//! Provides the ability for Xavier to navigate and query contexts specifically
//! by dates, time ranges, and timeline events. This acts as the "Cognitive Calendar",
//! allowing the LLM to understand the lifecycle of tasks and rebuild the state
//! of the context at a given point in time.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::memory::qmd_memory::QmdMemory;
use crate::memory::schema::MemoryQueryFilters;
use crate::memory::virtual_memory::{MemoryReference, VirtualMemory};

/// A request to navigate the cognitive calendar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineQuery {
    pub query: String,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub agent_id: Option<String>,
    pub limit: usize,
}

/// A slice of context from a specific point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSlice {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub memories: Vec<MemoryReference>,
    pub timeline_events: Vec<TimelineEventSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEventSummary {
    pub timestamp: DateTime<Utc>,
    pub operation: String,
    pub summary: String,
    pub agent_id: String,
}

pub struct TimelineEngine {
    memory: Arc<QmdMemory>,
}

impl TimelineEngine {
    pub fn new(memory: Arc<QmdMemory>) -> Self {
        Self { memory }
    }

    /// Retrieve a slice of context restricted to a specific time range.
    /// This simulates "time travel" by only looking at memories created
    /// or modified within that window, alongside the raw audit events.
    pub async fn get_time_slice(&self, query: &TimelineQuery) -> Result<TimeSlice> {
        let start = query
            .start_date
            .unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));
        let end = query.end_date.unwrap_or_else(Utc::now);

        // 1. Fetch memory references
        let filters = MemoryQueryFilters {
            recorded_after: Some(start.to_rfc3339()),
            recorded_before: Some(end.to_rfc3339()),
            ..Default::default()
        };

        let vm = VirtualMemory::new(Arc::clone(&self.memory), None);
        let entries = vm
            .page_in_filtered(&query.query, query.limit, Some(&filters))
            .await?;
        let memories: Vec<MemoryReference> =
            entries.into_iter().map(|e| e.to_reference()).collect();

        // 2. Fetch timeline audit events (if supported by backend)
        let since_iso = start.to_rfc3339();
        let mut timeline_events = Vec::new();
        let ws = self.memory.workspace_id();

        if let Some(store) = self.memory.store().await {
            if let Ok(events) = store.list_timeline_events(ws, &since_iso).await {
                for ev in events {
                    if let Ok(ts) = DateTime::parse_from_rfc3339(&ev.timestamp) {
                        let ts_utc = ts.with_timezone(&Utc);
                        if ts_utc <= end {
                            timeline_events.push(TimelineEventSummary {
                                timestamp: ts_utc,
                                operation: ev.event_type.clone(),
                                summary: ev
                                    .payload
                                    .get("summary")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                agent_id: ev.agent_id.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Sort events chronologically
        timeline_events.sort_by_key(|a| a.timestamp);
        timeline_events.truncate(query.limit);

        Ok(TimeSlice {
            period_start: start,
            period_end: end,
            memories,
            timeline_events,
        })
    }
}
