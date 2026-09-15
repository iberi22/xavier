//! Unit tests for memory manager types and functions.

use crate::memory::manager::types::*;
use crate::memory::qmd_memory::MemoryDocument;

#[test]
fn test_priority_from_metadata() {
    let critical_meta = serde_json::json!({"memory_priority": "critical"});
    assert_eq!(
        MemoryPriority::from_metadata(&critical_meta),
        MemoryPriority::Critical
    );

    let default_meta = serde_json::json!({});
    assert_eq!(
        MemoryPriority::from_metadata(&default_meta),
        MemoryPriority::Medium
    );
}

#[test]
fn test_gestalt_bus_priority_classification() {
    let bus_meta = serde_json::json!({"gestalt_context": "bus"});
    assert_eq!(
        MemoryPriority::from_metadata(&bus_meta),
        MemoryPriority::Ephemeral
    );

    let bus_path = "gestalt/bus/executions/2026-09-15T01-36-16-cron_test";
    let empty_meta = serde_json::json!({});
    assert_eq!(
        MemoryPriority::from_metadata_and_path(&empty_meta, bus_path),
        MemoryPriority::Ephemeral
    );
}

#[tokio::test]
async fn test_gestalt_bus_quota_bounding() {
    use crate::memory::qmd_memory::QmdMemory;
    use std::sync::Arc;

    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let qmd = Arc::new(QmdMemory::new(docs));

    // Insert 250 distinct bus execution events
    for i in 0..250 {
        let path = format!("gestalt/bus/executions/2026-09-15T01-{:04}", i);
        let content = format!("Execution log tick {}", i);
        let meta = serde_json::json!({"gestalt_context": "bus"});
        let _ = crate::memory::qmd::writer::add_document_typed(
            &qmd,
            path,
            content,
            meta,
            None,
        )
        .await;
    }

    let all_docs = qmd.all_documents().await;
    let bus_count = all_docs
        .iter()
        .filter(|d| crate::memory::qmd::writer::is_gestalt_bus_execution(&d.path, &d.metadata))
        .count();

    // Verify bounded size is <= 200 (max active execution events)
    assert!(
        bus_count <= 200,
        "Expected bus execution count <= 200, got {}",
        bus_count
    );
}

#[tokio::test]
async fn test_gestalt_bus_7day_retention_cleanup() {
    use crate::memory::manager::MemoryManager;
    use crate::memory::qmd_memory::QmdMemory;
    use std::sync::Arc;

    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let qmd = Arc::new(QmdMemory::new(docs));
    let manager = MemoryManager::new(qmd.clone(), None);

    // Old bus event (> 7 days old)
    let old_date = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
    let old_meta = serde_json::json!({
        "gestalt_context": "bus",
        "created_at": old_date,
        "updated_at": old_date,
    });
    let _ = crate::memory::qmd::writer::add_document_typed(
        &qmd,
        "gestalt/bus/executions/old_event".to_string(),
        "Old execution log".to_string(),
        old_meta,
        None,
    )
    .await;

    // Standard memory (must be preserved)
    let std_meta = serde_json::json!({
        "kind": "fact",
        "created_at": old_date,
    });
    let _ = crate::memory::qmd::writer::add_document_typed(
        &qmd,
        "user/profile/bela".to_string(),
        "Important user fact".to_string(),
        std_meta,
        None,
    )
    .await;

    let pruned = manager.prune_expired_bus_events().await.unwrap();
    assert_eq!(pruned, 1, "Expected 1 old bus event to be pruned");

    let remaining = qmd.all_documents().await;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].path, "user/profile/bela");
}

#[test]
fn test_quality_calculation() {
    let doc = MemoryDocument {
        id: Some("test".to_string()),
        path: "test/path".to_string(),
        content: "Test content".to_string(),
        metadata: serde_json::json!({"kind": "fact"}),
        content_vector: Some(vec![0.0; 384]),
        embedding: vec![0.0; 384],
        ..Default::default()
    };

    let quality = MemoryQuality::calculate(
        &doc,
        MemoryPriority::Medium,
        5,
        Some(chrono::Utc::now()),
        true,
    );

    assert!(quality.overall >= 0.0 && quality.overall <= 1.0);
    assert!(quality.accuracy_score == 1.0); // verified = true
}

#[test]
fn test_decay_calculation() {
    // Critical should not decay
    assert!((MemoryPriority::Critical.decay_base() - 1.0).abs() < 0.001);

    // Ephemeral decays fast
    assert!(MemoryPriority::Ephemeral.decay_base() < 0.6);
}

#[tokio::test]
async fn test_consolidation_signature_no_collision_on_equal_length() {
    use crate::memory::manager::MemoryManager;
    use crate::memory::qmd_memory::QmdMemory;
    use std::sync::Arc;

    let docs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let qmd = Arc::new(QmdMemory::new(docs));
    let manager = MemoryManager::new(qmd, None);

    let doc_a = MemoryDocument {
        id: Some("doc_a".to_string()),
        path: "a.md".to_string(),
        content: "aaaa".to_string(), // length 4
        metadata: serde_json::json!({"kind": "note"}),
        ..Default::default()
    };

    let doc_b = MemoryDocument {
        id: Some("doc_b".to_string()),
        path: "b.md".to_string(),
        content: "bbbb".to_string(), // length 4
        metadata: serde_json::json!({"kind": "note"}),
        ..Default::default()
    };

    let sig_a = manager.create_consolidation_signature(&doc_a);
    let sig_b = manager.create_consolidation_signature(&doc_b);

    assert_ne!(
        sig_a, sig_b,
        "Signatures must differ for different documents of equal length"
    );
}
