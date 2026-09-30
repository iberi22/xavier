use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryQueryFilters {
    pub kinds: Option<Vec<crate::memory::schema::MemoryKind>>,
    pub evidence_kinds: Option<Vec<crate::memory::schema::EvidenceKind>>,
    pub org_id: Option<String>,
    pub workspace_id: Option<String>,
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub scope: Option<String>,
    pub retrieval_scope: Option<crate::memory::schema::RetrievalScope>,
    pub source_app: Option<String>,
    pub source_type: Option<String>,
    pub repo_url: Option<String>,
    pub file_path: Option<String>,
    pub symbol: Option<String>,
    pub url: Option<String>,
    pub message_id: Option<String>,
    pub topic_key: Option<String>,
    pub observed_after: Option<String>,
    pub observed_before: Option<String>,
}
