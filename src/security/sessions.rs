//! Ephemeral Session Management for Frontend Access
//!
//! Issues short-lived, temporary session tokens for frontends to securely
//! interact with the Xavier backend without exposure of root tokens.

use chrono::{DateTime, Utc, Duration};
use dashmap::DashMap;
use uuid::Uuid;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct EphemeralSession {
    pub id: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl EphemeralSession {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

pub struct SessionManager {
    sessions: Arc<DashMap<String, EphemeralSession>>,
    default_ttl: Duration,
}

impl SessionManager {
    pub fn new(ttl_minutes: i64) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            default_ttl: Duration::minutes(ttl_minutes),
        }
    }

    /// Create a new ephemeral session
    pub fn create_session(&self) -> EphemeralSession {
        let id = format!("sess_{}", Uuid::new_v4());
        let now = Utc::now();
        let session = EphemeralSession {
            id: id.clone(),
            expires_at: now + self.default_ttl,
            created_at: now,
        };
        self.sessions.insert(id, session.clone());
        session
    }

    /// Validate a session ID
    pub fn validate_session(&self, id: &str) -> bool {
        if let Some(session) = self.sessions.get(id) {
            if !session.is_expired() {
                return true;
            }
        }
        // Cleanup expired session if found
        self.sessions.remove(id);
        false
    }

    /// Cleanup all expired sessions
    pub fn cleanup_expired(&self) -> usize {
        let now = Utc::now();
        let mut count = 0;
        self.sessions.retain(|_, session| {
            let expired = session.expires_at < now;
            if expired {
                count += 1;
            }
            !expired
        });
        count
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new(60) // 1 hour default TTL
    }
}
