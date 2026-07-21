// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Ephemeral Session Management for Frontend Access
//!
//! Issues short-lived, temporary session tokens for frontends to securely
//! interact with the Xavier backend without exposure of root tokens.

use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

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
    sessions: Arc<RwLock<HashMap<String, EphemeralSession>>>,
    default_ttl: Duration,
}

impl SessionManager {
    pub fn new(ttl_minutes: i64) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
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
        self.sessions.write().insert(id, session.clone());
        session
    }

    /// Validate a session ID
    pub fn validate_session(&self, id: &str) -> bool {
        if let Some(session) = self.sessions.read().get(id) {
            if !session.is_expired() {
                return true;
            }
        }
        // Cleanup expired session if found
        self.sessions.write().remove(id);
        false
    }

    /// Cleanup all expired sessions
    pub fn cleanup_expired(&self) -> usize {
        let now = Utc::now();
        let mut count = 0;
        self.sessions.write().retain(|_, session| {
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
