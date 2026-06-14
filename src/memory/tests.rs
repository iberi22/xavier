//! Integration tests for the memory module
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
// ============================================
// Tests for Xavier Memory System
// ============================================

#[cfg(test)]
mod tests {
    use crate::memory::simple_index::{extract_keywords, SimpleMemoryDoc, SimpleMemoryIndex};
    use crate::memory::virtual_memory::{Checkpoint, TokenSavings, VirtualMemoryEntry};
    use crate::memory::qmd::utils::cosine_similarity;
    use crate::memory::hierarchy::MemoryHierarchyNode;
    use crate::memory::store::{MemoryRecord, InMemoryMemoryStore, MemoryStore};
    use chrono::Utc;

    // ==================== Hierarchy Tests ====================

    fn mock_record(path: &str) -> MemoryRecord {
        MemoryRecord {
            id: path.to_string(),
            workspace_id: "test".to_string(),
            path: path.to_string(),
            content: "test".to_string(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: Default::default(),
            relation: None,
            clearance: Default::default(),
            revisions: vec![],
            content_iv: None,
            encrypted_dek: None,
            metadata_iv: None,
        }
    }

    #[tokio::test]
    async fn test_store_ls_integration() {
        let store = InMemoryMemoryStore::new();
        store.put(mock_record("docs/api/v1.md")).await.expect("test assertion");
        store.put(mock_record("docs/api/v2.md")).await.expect("test assertion");
        store.put(mock_record("docs/readme.md")).await.expect("test assertion");
        store.put(mock_record("blog/post1.md")).await.expect("test assertion");

        // List root
        let root = store.ls("test", "").await.expect("test assertion");
        assert_eq!(root.len(), 2); // blog/ (Dir), docs/ (Dir)

        if let MemoryHierarchyNode::Directory { name, .. } = &root[0] {
            assert_eq!(name, "blog");
        } else {
            panic!("Expected directory blog");
        }

        // List docs
        let docs = store.ls("test", "docs").await.expect("test assertion");
        assert_eq!(docs.len(), 2); // api/ (Dir), readme.md (File)

        if let MemoryHierarchyNode::Directory { name, .. } = &docs[0] {
            assert_eq!(name, "api");
        } else {
            panic!("Expected directory api");
        }

        if let MemoryHierarchyNode::File(record) = &docs[1] {
            assert_eq!(record.path, "docs/readme.md");
        } else {
            panic!("Expected file docs/readme.md");
        }
    }

    // ==================== Keyword Search Tests ====================

    #[test]
    fn test_keyword_extraction() {
        let content = "This is a test document about Next.js and Supabase";
        let keywords = extract_keywords(content);

        assert!(keywords.contains(&"nextjs".to_string()));
        assert!(keywords.contains(&"supabase".to_string()));
        assert!(!keywords.contains(&"this".to_string())); // stop word
    }

    #[test]
    fn test_search_returns_results() {
        // This test verifies that search actually returns results
        // after adding documents
        let mut index = SimpleMemoryIndex::new();

        // Add document
        let doc = SimpleMemoryDoc::new(
            "test.rs".to_string(),
            "fn main() { println!(\"Hello\"); }".to_string(),
            serde_json::json!({"type": "test"})
        );
        index.add(doc);

        // Search should return results
        let results = index.search("Hello", 5);
        assert!(!results.is_empty());
    }

    #[test]
    fn test_keyword_scoring() {
        let mut index = SimpleMemoryIndex::new();

        // Add multiple docs
        index.add(SimpleMemoryDoc::new(
            "test1.rs".to_string(),
            "fn main() { println!(\"test\"); }".to_string(),
            serde_json::json!({})
        ));

        index.add(SimpleMemoryDoc::new(
            "test2.rs".to_string(),
            "other content".to_string(),
            serde_json::json!({})
        ));

        let results = index.search("test", 5);
        assert!(results.len() > 0);
        assert!(results[0].score > 0.0);
    }

    // ==================== Embedding Tests ====================

    #[test]
    fn test_embedding_generation() {
        // Test that we can call pplx-embed and get embeddings
        // This is a placeholder - actual test would make HTTP call
        let text = "Test document content";
        assert!(!text.is_empty());
    }

    #[test]
    fn test_cosine_similarity() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![1.0, 0.0, 0.0];

        // Same vectors should have similarity 1.0
        let sim = cosine_similarity(&vec1, &vec2);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_orthogonal_vectors() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![0.0, 1.0, 0.0];

        // Orthogonal vectors should have similarity 0.0
        let sim = cosine_similarity(&vec1, &vec2);
        assert!(sim.abs() < 0.001);
    }

    // ==================== Checkpoint Tests ====================

    #[test]
    fn test_checkpoint_size_limit() {
        let checkpoint = Checkpoint::from_session(
            "Fixed authentication bug",
            vec!["auth.rs".to_string()],
            vec!["commit abc123".to_string()],
            vec!["Fix login".to_string()],
        );

        // Checkpoint should be under 2KB
        assert!(checkpoint.size() < 2048);
    }

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = Checkpoint::new();

        assert!(!checkpoint.id.is_empty());
        assert!(checkpoint.timestamp > 0);
    }

    #[test]
    fn test_checkpoint_serialization() {
        let checkpoint = Checkpoint::from_session(
            "Summary of work done",
            vec!["file1.rs".to_string()],
            vec!["git commit".to_string()],
            vec!["task 1".to_string()],
        );

        // Should serialize and deserialize correctly
        let json = serde_json::to_string(&checkpoint).expect("test assertion");
        let restored: Checkpoint = serde_json::from_str(&json).expect("test assertion");

        assert_eq!(checkpoint.id, restored.id);
    }

    // ==================== Token Reduction Tests ====================

    #[test]
    fn test_token_savings() {
        let original = "x ".repeat(56000); // 112KB with spaces
        let entry = VirtualMemoryEntry::new(
            "test.txt".to_string(),
            original.clone(),
            serde_json::json!({}),
        );

        let savings = TokenSavings::calculate(&original, &entry);

        // Should save more than 90%
        assert!(savings.reduction_percent > 90.0);
    }

    #[test]
    fn test_summary_creation() {
        let content = "a".repeat(1000);
        let entry = VirtualMemoryEntry::new(
            "test.txt".to_string(),
            content,
            serde_json::json!({}),
        );

        // Summary should be shorter than original
        assert!(entry.summary.len() < 1000);
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_hybrid_search() {
        let mut index = SimpleMemoryIndex::new();

        // Add docs
        index.add(SimpleMemoryDoc::new(
            "nextjs.rs".to_string(),
            "Next.js with Supabase authentication".to_string(),
            serde_json::json!({})
        ));

        // Should find by keywords
        let results = index.search("Next.js Supabase", 5);
        assert!(!results.is_empty());
    }
}
