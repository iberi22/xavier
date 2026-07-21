// SPDX-License-Identifier: MIT OR LICENSE-MESH
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
