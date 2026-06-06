use serde::{Deserialize, Serialize};

/// Action taken by memory manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryManagementAction {
    Decayed {
        doc_id: String,
        old_relevance: f32,
        new_relevance: f32,
    },
    Consolidated {
        doc_ids: Vec<String>,
        into_doc_id: String,
    },
    Evicted {
        doc_id: String,
        reason: String,
        priority: String,
    },
    Compressed {
        doc_id: String,
        old_size: u64,
        new_size: u64,
    },
    Archived {
        doc_id: String,
        archive_path: String,
    },
    Promoted {
        doc_id: String,
        old_priority: String,
        new_priority: String,
    },
    Demoted {
        doc_id: String,
        old_priority: String,
        new_priority: String,
    },
}

/// Result of a management operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagementResult {
    pub actions: Vec<MemoryManagementAction>,
    pub documents_affected: usize,
    pub bytes_freed: u64,
}

/// Legacy action types for backwards compatibility with existing code
#[derive(Debug, Clone)]
pub enum MemoryAction {
    Keep,
    Compress {
        doc_id: String,
        reason: String,
    },
    Delete {
        doc_id: String,
        reason: String,
    },
    Update {
        doc_id: String,
        new_content: String,
    },
    Consolidate {
        doc_ids: Vec<String>,
        reason: String,
    },
    Curate {
        doc_id: String,
    },
}
