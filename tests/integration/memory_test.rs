//! Memory Module Tests - aligned with the current QmdMemory implementation.

#[cfg(test)]
mod memory_manager_tests {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use xavier::memory::manager::{
        MemoryManagementAction, MemoryManager, MemoryManagerConfig, MemoryPriority,
    };
    use xavier::memory::qmd_memory::{MemoryDocument, QmdMemory};

    fn empty_memory() -> Arc<QmdMemory> {
        Arc::new(QmdMemory::new(Arc::new(RwLock::new(Vec::new()))))
    }

    #[tokio::test]
    async fn test_memory_manager_creation() {
        let manager = MemoryManager::new(empty_memory(), None);

        assert_eq!(manager.config().max_documents, 10_000);
        assert_eq!(manager.config().max_storage_bytes, 500 * 1024 * 1024);
        assert!(manager.config().auto_consolidate_enabled);
    }

    #[tokio::test]
    async fn test_get_stats_empty() {
        let manager = MemoryManager::new(empty_memory(), None);

        let stats = manager.get_stats().await.expect("test assertion");
        assert_eq!(stats.total_documents, 0);
        assert_eq!(stats.total_size_bytes, 0);
        assert_eq!(stats.low_quality_count, 0);
    }

    #[tokio::test]
    async fn test_get_memories_by_priority_empty() {
        let manager = MemoryManager::new(empty_memory(), None);

        let memories = manager
            .get_memories_by_priority(MemoryPriority::Medium)
            .await
            .expect("test assertion");
        assert!(memories.is_empty());
    }

    #[tokio::test]
    async fn test_consolidate_memories_recommends_and_removes_duplicate() {
        let docs = Arc::new(RwLock::new(Vec::new()));
        {
            let mut docs_guard = docs.write().await;
            docs_guard.push(MemoryDocument {
                id: Some("doc_old".to_string()),
                path: "memory/doc_old".to_string(),
                content: "Same content".to_string(),
                metadata: serde_json::json!({"kind": "fact", "memory_priority": "medium"}),
                content_vector: None,
                embedding: vec![],
                ..Default::default()
            });
            docs_guard.push(MemoryDocument {
                id: Some("doc_new".to_string()),
                path: "memory/doc_new".to_string(),
                content: "same    content".to_string(),
                metadata: serde_json::json!({"kind": "fact", "memory_priority": "medium"}),
                content_vector: None,
                embedding: vec![],
                ..Default::default()
            });
        }

        let manager = MemoryManager::new(Arc::new(QmdMemory::new(docs)), None);
        let result = manager
            .consolidate_memories()
            .await
            .expect("test assertion");

        assert_eq!(result.documents_affected, 1);
        assert_eq!(result.actions.len(), 1);
        assert!(matches!(
            result.actions[0],
            MemoryManagementAction::Consolidated { .. }
        ));

        let remaining = manager.get_stats().await.expect("test assertion");
        assert_eq!(remaining.total_documents, 1);
    }

    #[tokio::test]
    async fn test_evict_low_quality_removes_low_priority_docs() {
        use xavier::memory::access;
        use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};

        let docs = Arc::new(RwLock::new(vec![MemoryDocument {
            id: Some("doc_low".to_string()),
            path: "memory/doc_low".to_string(),
            content: "tiny".to_string(),
            metadata: serde_json::json!({"memory_priority": "low"}),
            content_vector: None,
            embedding: vec![],
            ..Default::default()
        }]));

        let config = MemoryManagerConfig {
            quality_threshold: 0.99,
            ..MemoryManagerConfig::default()
        };
        let manager =
            MemoryManager::with_config(Arc::new(QmdMemory::new(Arc::clone(&docs))), None, config);
        let result = manager.evict_low_quality().await.expect("test assertion");

        // Low priority alone is not evidence of disuse.
        assert_eq!(result.documents_affected, 0);
        assert!(result.actions.is_empty());
        assert_eq!(result.bytes_freed, 0);
        assert_eq!(docs.read().await.len(), 1);
        assert_eq!(docs.read().await[0].id.as_deref(), Some("doc_low"));

        // Match the store-backed fixture in manager::eviction: seed the clock
        // before construction, which preserves it with INSERT OR IGNORE.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("eviction_disuse.sqlite3");
        let started =
            chrono::Utc::now() - chrono::Duration::days(access::MIN_OBSERVATION_DAYS + 10);
        let conn = rusqlite::Connection::open(&path).expect("open new db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS maintenance_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .expect("create maintenance_meta");
        conn.execute(
            "INSERT OR IGNORE INTO maintenance_meta (key, value, updated_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                access::OBSERVATION_STARTED_AT_KEY,
                started.to_rfc3339(),
                started.to_rfc3339()
            ],
        )
        .expect("seed clock");
        conn.close().expect("close");
        let store = Arc::new(
            VecSqliteMemoryStore::new(VecSqliteStoreConfig {
                path,
                embedding_dimensions: 8,
            })
            .await
            .expect("store construction"),
        );
        manager.memory().set_store(store).await;
        // An embedding makes the never-accessed document observable.
        docs.write().await[0].embedding = vec![0.1, 0.2];
        let expected_bytes = docs.read().await[0].estimated_bytes();
        let result = manager.evict_low_quality().await.expect("test assertion");

        assert_eq!(result.documents_affected, 1);
        assert_eq!(result.actions.len(), 1);
        assert_eq!(result.bytes_freed, expected_bytes);
        assert!(matches!(
            &result.actions[0],
            MemoryManagementAction::Evicted { doc_id, priority, .. }
                if doc_id == "doc_low" && priority == "low"
        ));
        assert!(docs.read().await.is_empty());
    }
}

#[cfg(test)]
mod memory_stats_tests {
    use std::collections::HashMap;
    use xavier::memory::manager::MemoryStats;

    #[test]
    fn test_memory_stats_creation() {
        let stats = MemoryStats {
            total_documents: 100,
            total_size_bytes: 50000,
            by_priority: HashMap::from([(String::from("medium"), 90)]),
            by_quality_bucket: HashMap::from([(String::from("high"), 75)]),
            low_quality_count: 10,
            ephemeral_count: 3,
            decayed_count: 2,
        };

        assert_eq!(stats.total_documents, 100);
        assert_eq!(stats.total_size_bytes, 50000);
        assert_eq!(stats.by_priority["medium"], 90);
        assert_eq!(stats.by_quality_bucket["high"], 75);
    }
}

#[cfg(test)]
mod memory_action_tests {
    use xavier::memory::manager::MemoryAction;

    #[test]
    fn test_memory_action_variants() {
        let keep = MemoryAction::Keep;
        assert!(matches!(keep, MemoryAction::Keep));

        let compress = MemoryAction::Compress {
            doc_id: "doc1".to_string(),
            reason: "redundant".to_string(),
        };
        assert!(matches!(compress, MemoryAction::Compress { .. }));

        let delete = MemoryAction::Delete {
            doc_id: "doc2".to_string(),
            reason: "outdated".to_string(),
        };
        assert!(matches!(delete, MemoryAction::Delete { .. }));

        let update = MemoryAction::Update {
            doc_id: "doc3".to_string(),
            new_content: "new content".to_string(),
        };
        assert!(matches!(update, MemoryAction::Update { .. }));

        let consolidate = MemoryAction::Consolidate {
            doc_ids: vec!["doc4".to_string()],
            reason: "merge".to_string(),
        };
        assert!(matches!(consolidate, MemoryAction::Consolidate { .. }));
    }
}
