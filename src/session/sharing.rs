//! Session sharing and export/import utilities
//!
//! Provides functionality for bundling session documents for transport
//! and ingesting session bundles from other Xavier instances.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};
use crate::memory::schema::{matches_filters, MemoryKind, MemoryQueryFilters};

/// A portable bundle containing all documents and metadata for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBundle {
    pub session_id: String,
    pub documents: Vec<MemoryDocument>,
    pub exported_at: i64,
}

/// A portable bundle containing the optimized context state for a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBundle {
    pub session_id: String,
    pub optimized_context: String,
    pub depth: String,
    pub created_at: i64,
}

/// Export a session into a portable bundle
pub async fn export_session(memory: &QmdMemory, session_id: &str) -> Result<SessionBundle> {
    let filters = MemoryQueryFilters {
        session_id: Some(session_id.to_string()),
        kinds: Some(vec![MemoryKind::Session]),
        ..Default::default()
    };

    let mut documents: Vec<MemoryDocument> = memory
        .all_documents()
        .await
        .into_iter()
        .filter(|doc| {
            matches_filters(
                &doc.path,
                &doc.metadata,
                memory.workspace_id(),
                Some(&filters),
            )
        })
        .take(1000)
        .collect();

    // Auto-redact sensitive PII data before exporting/sharing
    let redaction_engine = crate::security::redaction::RedactionEngine::default();
    for doc in &mut documents {
        doc.content = redaction_engine.redact(&doc.content);
    }

    Ok(SessionBundle {
        session_id: session_id.to_string(),
        documents,
        exported_at: chrono::Utc::now().timestamp(),
    })
}

/// Import a session bundle into the local memory store
pub async fn import_session(memory: &Arc<QmdMemory>, bundle: SessionBundle) -> Result<()> {
    for doc in bundle.documents {
        // Ensure path and metadata are consistent with local storage
        let path = doc.path.clone();
        let content = doc.content.clone();
        let metadata = doc.metadata.clone();

        memory
            .add_document_typed(path, content, metadata, None)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::qmd_memory::QmdMemory;
    use crate::memory::schema::{MemoryKind, MemoryNamespace, TypedMemoryPayload};
    use crate::memory::store::InMemoryMemoryStore;
    use tokio::sync::RwLock;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_session_export_import() {
        let docs = Arc::new(RwLock::new(vec![]));
        let memory = Arc::new(QmdMemory::new_with_workspace(
            docs,
            "test-workspace".to_string(),
        ));
        let store = Arc::new(InMemoryMemoryStore::new());
        memory.set_store(store).await;

        // Add some session documents
        let session_id = "test-session-123";
        let typed = Some(TypedMemoryPayload {
            kind: Some(MemoryKind::Session),
            namespace: Some(MemoryNamespace {
                session_id: Some(session_id.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        memory
            .add_document_typed(
                format!("sessions/{}/1", session_id),
                "message 1".to_string(),
                serde_json::json!({}),
                typed.clone(),
            )
            .await
            .unwrap();

        memory
            .add_document_typed(
                format!("sessions/{}/2", session_id),
                "message 2".to_string(),
                serde_json::json!({}),
                typed,
            )
            .await
            .unwrap();

        // Export
        let bundle = export_session(&memory, session_id).await.unwrap();
        assert_eq!(bundle.session_id, session_id);
        assert_eq!(bundle.documents.len(), 2);

        // Import into a new memory store
        let docs2 = Arc::new(RwLock::new(vec![]));
        let memory2 = Arc::new(QmdMemory::new_with_workspace(
            docs2,
            "other-workspace".to_string(),
        ));
        let store2 = Arc::new(InMemoryMemoryStore::new());
        memory2.set_store(store2).await;

        import_session(&memory2, bundle).await.unwrap();

        let imported_docs = memory2.all_documents().await;
        assert_eq!(imported_docs.len(), 2);
        assert!(imported_docs.iter().any(|d| d.content == "message 1"));
        assert!(imported_docs.iter().any(|d| d.content == "message 2"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_export_session_redacts_pii() {
        let docs = Arc::new(RwLock::new(vec![]));
        let memory = Arc::new(QmdMemory::new_with_workspace(
            docs,
            "test-workspace".to_string(),
        ));
        let store = Arc::new(InMemoryMemoryStore::new());
        memory.set_store(store).await;

        let session_id = "test-session-pii";
        let typed = Some(TypedMemoryPayload {
            kind: Some(MemoryKind::Session),
            namespace: Some(MemoryNamespace {
                session_id: Some(session_id.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        });

        memory
            .add_document_typed(
                format!("sessions/{}/1", session_id),
                "Contact john.doe@example.com for secret details".to_string(),
                serde_json::json!({}),
                typed,
            )
            .await
            .unwrap();

        let bundle = export_session(&memory, session_id).await.unwrap();
        assert_eq!(bundle.documents.len(), 1);
        let content = &bundle.documents[0].content;
        assert!(!content.contains("john.doe@example.com"));
        assert!(content.contains("[EMAIL]"));
    }

    #[test]
    fn test_context_bundle_serialization() {
        let bundle = ContextBundle {
            session_id: "ctx-sess-1".to_string(),
            optimized_context: "High density summary context".to_string(),
            depth: "deep".to_string(),
            created_at: 1700000000,
        };

        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("ctx-sess-1"));
        assert!(json.contains("High density summary context"));

        let deserialized: ContextBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.session_id, "ctx-sess-1");
        assert_eq!(deserialized.depth, "deep");
    }
}
