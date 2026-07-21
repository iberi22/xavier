//! Type definitions for SQLite vector store
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::memory::store::SessionTokenRecord;

pub(crate) struct SessionTokenRow {
    pub token: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl From<SessionTokenRow> for SessionTokenRecord {
    fn from(value: SessionTokenRow) -> Self {
        Self {
            token: value.token,
            created_at: value.created_at,
            expires_at: value.expires_at,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FusionSource {
    Vector,
    Fts,
    Kg,
}

impl FusionSource {
    /// Default weight.
    pub fn default_weight(self) -> f32 {
        use crate::memory::sqlite_vec_store::config::*;
        match self {
            Self::Vector => DEFAULT_VECTOR_WEIGHT,
            Self::Fts => DEFAULT_FTS_WEIGHT,
            Self::Kg => DEFAULT_KG_WEIGHT,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    pub value: String,
    pub entity_type: &'static str,
    pub relation_type: &'static str,
}

#[derive(Debug, Clone)]
pub struct TimelineEventRecord {
    pub id: String,
    pub agent_id: String,
    pub timestamp: String,
    pub operation: String,
    pub prev_hash: Option<String>,
    pub curr_hash: String,
}
