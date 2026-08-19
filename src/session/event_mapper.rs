//! Session event mapping
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::session::types::{SessionEvent, SessionEventType};
use chrono::{DateTime, Utc};
use tracing::info;

/// A single entry in a panel/thread conversation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PanelThreadEntry {
    pub role: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub session_id: String,
    pub event_type: String,
}

/// Maps a SessionEvent to a PanelThreadEntry.
/// Returns None for SessionStart/SessionEnd events (which are metadata only).
pub fn map_to_panel_thread(event: SessionEvent) -> Option<PanelThreadEntry> {
    let role = match event.event_type {
        SessionEventType::Message => "user",
        SessionEventType::ToolCall => "tool",
        SessionEventType::ToolResult => "assistant",
        SessionEventType::SessionStart => return None,
        SessionEventType::SessionEnd => return None,
        SessionEventType::Error => "system",
    };

    let content = event.content.clone().unwrap_or_default();
    if content.is_empty() {
        return None;
    }

    info!(
        session_id = %event.session_id,
        role = %role,
        content_len = content.len(),
        "mapping session event to panel thread"
    );

    Some(PanelThreadEntry {
        role: role.to_string(),
        content,
        timestamp: event.timestamp,
        session_id: event.session_id,
        event_type: serde_json::to_string(&event.event_type).unwrap_or_default(),
    })
}

impl PanelThreadEntry {
    /// From session event.
    pub fn from_session_event(event: &SessionEvent) -> Option<Self> {
        let role = match event.event_type {
            SessionEventType::Message => "user",
            SessionEventType::ToolCall => "tool",
            SessionEventType::ToolResult => "assistant",
            SessionEventType::SessionStart => return None,
            SessionEventType::SessionEnd => return None,
            SessionEventType::Error => "system",
        };

        let content = event.content.clone().unwrap_or_default();
        if content.is_empty() {
            return None;
        }

        Some(Self {
            role: role.to_string(),
            content,
            timestamp: event.timestamp,
            session_id: event.session_id.clone(),
            event_type: serde_json::to_string(&event.event_type).unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_map_to_panel_thread_message_user() {
        let event = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("User query content".to_string()),
            metadata: None,
        };

        let entry = map_to_panel_thread(event).expect("mapped entry expected");
        assert_eq!(entry.role, "user");
        assert_eq!(entry.content, "User query content");
        assert_eq!(entry.session_id, "sess-123");
    }

    #[test]
    fn test_map_to_panel_thread_tool_call() {
        let event = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::ToolCall,
            timestamp: Utc::now(),
            content: Some("Call tool_a".to_string()),
            metadata: None,
        };

        let entry = map_to_panel_thread(event).expect("mapped entry expected");
        assert_eq!(entry.role, "tool");
        assert_eq!(entry.content, "Call tool_a");
    }

    #[test]
    fn test_map_to_panel_thread_tool_result() {
        let event = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::ToolResult,
            timestamp: Utc::now(),
            content: Some("Tool output ok".to_string()),
            metadata: None,
        };

        let entry = map_to_panel_thread(event).expect("mapped entry expected");
        assert_eq!(entry.role, "assistant");
        assert_eq!(entry.content, "Tool output ok");
    }

    #[test]
    fn test_map_to_panel_thread_error() {
        let event = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::Error,
            timestamp: Utc::now(),
            content: Some("Fatal execution failure".to_string()),
            metadata: None,
        };

        let entry = map_to_panel_thread(event).expect("mapped entry expected");
        assert_eq!(entry.role, "system");
        assert_eq!(entry.content, "Fatal execution failure");
    }

    #[test]
    fn test_map_to_panel_thread_session_start_end_returns_none() {
        let start_event = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::SessionStart,
            timestamp: Utc::now(),
            content: Some("Session initialized".to_string()),
            metadata: None,
        };
        assert!(map_to_panel_thread(start_event).is_none());

        let end_event = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::SessionEnd,
            timestamp: Utc::now(),
            content: Some("Session terminated".to_string()),
            metadata: None,
        };
        assert!(map_to_panel_thread(end_event).is_none());
    }

    #[test]
    fn test_map_to_panel_thread_empty_content_returns_none() {
        let none_content = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: None,
            metadata: None,
        };
        assert!(map_to_panel_thread(none_content).is_none());

        let empty_content = SessionEvent {
            session_id: "sess-123".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("".to_string()),
            metadata: None,
        };
        assert!(map_to_panel_thread(empty_content).is_none());
    }
}
