//! Workspace templates for project scaffolding
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use super::state::WorkspaceState;

pub async fn seed_workspace(workspace: &WorkspaceState) -> Result<()> {
    let seed_docs = [
        ( "system/xavier", "Xavier is the central memory system for SWAL agents. Use /memory/add to store, /memory/search to find, /memory/query for AI responses.", serde_json::json!({"type": "system", "tags": ["xavier", "memory"]}) ),
        ( "system/swal", "SouthWest AI Labs (SWAL) builds AI agents. BELA is the developer. Projects: Xavier (memory), ZeroClaw (runtime), ManteniApp (SaaS), Trading Bot.", serde_json::json!({"type": "company", "tags": ["swal", "company"]}) ),
        ( "docs/api", "Xavier API: POST /memory/add (content, path, metadata), POST /memory/search (query, limit), POST /memory/query (query). Auth: X-Xavier-Token header.", serde_json::json!({"type": "docs", "tags": ["api"]}) ),
    ];

    for (path, content, metadata) in seed_docs {
        if workspace.memory.get(path).await?.is_some() { continue; }
        let normalized = crate::memory::schema::normalize_metadata(path, metadata, &workspace.config().id, None)?;
        workspace.memory.add(crate::memory::qmd_memory::MemoryDocument {
            id: Some(ulid::Ulid::new().to_string()),
            path: path.to_string(),
            content: content.to_string(),
            metadata: normalized,
            content_vector: Some(Vec::new()),
            embedding: Vec::new(),
            parent_id: None,
            ..Default::default()
        }).await?;
    }
    Ok(())
}
