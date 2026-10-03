//! Session type definitions
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
pub use crate::domain::cycle_breaks::w30_09::{SessionEvent, SessionEventType};

impl SessionEvent {
    /// Content preview.
    pub fn content_preview(&self) -> String {
        self.content
            .as_ref()
            .map(|c| {
                if c.chars().count() > 200 {
                    format!("{}...", c.chars().take(200).collect::<String>())
                } else {
                    c.clone()
                }
            })
            .unwrap_or_default()
    }
}
