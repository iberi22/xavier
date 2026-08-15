//! Session indexer
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use tracing::info;

use crate::session::event_mapper::PanelThreadEntry;
use crate::session::types::SessionEvent;

/// Maps session events to PanelThreadEntry and indexes them into Xavier memory stores
pub struct SessionIndexer;

impl SessionIndexer {
    /// Map a session event to a thread entry (returns None for session start/end)
    pub fn index_event(event: &SessionEvent) -> Option<PanelThreadEntry> {
        let entry = PanelThreadEntry::from_session_event(event)?;

        info!(
            session_id = %event.session_id,
            role = %entry.role,
            content_len = entry.content.len(),
            "mapping session event"
        );

        Some(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::types::SessionEventType;
    use chrono::Utc;

    #[test]
    fn test_session_indexer_index_event_valid() {
        let event = SessionEvent {
            session_id: "idx-sess-1".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Indexer message content".to_string()),
            metadata: None,
        };

        let entry = SessionIndexer::index_event(&event);
        assert!(entry.is_some());
        let entry = entry.expect("valid entry expected");
        assert_eq!(entry.session_id, "idx-sess-1");
        assert_eq!(entry.role, "user");
        assert_eq!(entry.content, "Indexer message content");
    }

    #[test]
    fn test_session_indexer_index_event_none() {
        let event = SessionEvent {
            session_id: "idx-sess-2".to_string(),
            event_type: SessionEventType::SessionStart,
            timestamp: Utc::now(),
            content: Some("Start session".to_string()),
            metadata: None,
        };

        let entry = SessionIndexer::index_event(&event);
        assert!(entry.is_none());
    }
}
