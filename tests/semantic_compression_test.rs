//! Integration tests for hierarchical semantic memory compression

use chrono::{Duration, Utc};
use xavier::memory::compression::{
    DialogueTurn, SemanticCompressor, SemanticCompressorConfig,
};

#[test]
fn test_cosine_similarity_and_distance() {
    let vec_a = vec![1.0, 0.0, 0.0];
    let vec_b = vec![0.9, 0.1, 0.0];
    let vec_c = vec![0.0, 1.0, 0.0];

    let sim_ab = SemanticCompressor::calculate_cosine_similarity(&vec_a, &vec_b);
    let dist_ab = SemanticCompressor::calculate_cosine_distance(&vec_a, &vec_b);
    assert!(sim_ab > 0.85, "Expected high similarity (>0.85), got {}", sim_ab);
    assert_eq!((1.0 - sim_ab), dist_ab);

    let sim_ac = SemanticCompressor::calculate_cosine_similarity(&vec_a, &vec_c);
    assert!(sim_ac < 0.1, "Expected low similarity (<0.1), got {}", sim_ac);
}

#[test]
fn test_turn_clustering_by_similarity_threshold() {
    let compressor = SemanticCompressor::with_config(SemanticCompressorConfig {
        similarity_threshold: 0.85,
        max_cluster_size: 5,
        ..SemanticCompressorConfig::default()
    });

    // Turn 1 and 2 have vector embeddings with >0.85 similarity
    let turn1 = DialogueTurn::new("t1", "sess1", "user", "How do I configure SWAL node identity?", 0)
        .with_embedding(vec![0.9, 0.1, 0.0]);
    let turn2 = DialogueTurn::new("t2", "sess1", "assistant", "You configure SWAL node identity via the NodeIdentityVault.", 1)
        .with_embedding(vec![0.88, 0.12, 0.0]);

    // Turn 3 has orthogonal embedding (<0.85 similarity)
    let turn3 = DialogueTurn::new("t3", "sess1", "user", "What is the weather in Tokyo today?", 2)
        .with_embedding(vec![0.0, 0.0, 1.0]);

    let turns = vec![turn1, turn2, turn3];
    let clusters = compressor.cluster_turns(&turns);

    assert_eq!(clusters.len(), 2, "Expected 2 distinct clusters based on >0.85 similarity");
    assert_eq!(clusters[0].len(), 2, "First cluster should contain turn1 and turn2");
    assert_eq!(clusters[1].len(), 1, "Second cluster should contain turn3");
}

#[test]
fn test_key_entity_preservation() {
    let compressor = SemanticCompressor::new();

    let text = "User USR-9001 updated server NODE-AX4 on date 2025-03-01 for PROJ-SWAL using release v2.4.1.";
    let entities = SemanticCompressor::extract_key_entities(text);

    assert!(entities.contains(&"USR-9001".to_string()), "Should preserve USR-9001 entity ID");
    assert!(entities.contains(&"NODE-AX4".to_string()), "Should preserve NODE-AX4 entity ID");
    assert!(entities.contains(&"PROJ-SWAL".to_string()), "Should preserve PROJ-SWAL entity ID");
    assert!(entities.contains(&"v2.4.1".to_string()), "Should preserve v2.4.1 version entity");

    // Test full session compression entity preservation
    let turn1 = DialogueTurn::new("t1", "sess-entities", "user", "Issue reported for USR-9001 on NODE-AX4.", 0)
        .with_entities(vec!["USR-9001".to_string(), "NODE-AX4".to_string()]);
    let turn2 = DialogueTurn::new("t2", "sess-entities", "assistant", "Applied patch v2.4.1 for PROJ-SWAL.", 1);

    let result = compressor.compress_session("sess-entities", &[turn1, turn2]);

    assert!(result.preserved_entities.contains(&"USR-9001".to_string()));
    assert!(result.preserved_entities.contains(&"NODE-AX4".to_string()));
    assert!(result.preserved_entities.contains(&"PROJ-SWAL".to_string()));
    assert!(result.preserved_entities.contains(&"v2.4.1".to_string()));
}

#[test]
fn test_hierarchical_compression_and_ratios() {
    let compressor = SemanticCompressor::new();

    // Generate 20 turns simulating a longer dialogue thread
    let mut turns = Vec::new();
    for i in 0..20 {
        let role = if i % 2 == 0 { "user" } else { "assistant" };
        let content = format!(
            "Turn #{i}: Detailed explanation of database indexing for PROJ-{i} with node NODE-{i}. \
            We are configuring high availability parameters and testing performance under load.",
            i = i
        );
        let turn = DialogueTurn::new(format!("t{i}"), "session-100", role, content, i)
            .with_embedding(vec![0.85 + (i as f32 * 0.001), 0.1, 0.0]);
        turns.push(turn);
    }

    let result = compressor.compress_session("session-100", &turns);

    assert_eq!(result.original_turn_count, 20);
    assert!(result.cards.len() >= 2, "Expected Level 1 cards plus Level 2 overview card");
    assert!(result.original_char_count > result.compressed_char_count);
    assert!(result.overall_compression_ratio > 0.0);
    assert!(result.storage_bytes_saved > 0);

    // Verify Level 2 card exists
    let l2_card = result.cards.iter().find(|c| c.level == 2);
    assert!(l2_card.is_some(), "Expected Level-2 summary card");
    let overview = l2_card.unwrap();
    assert_eq!(overview.level, 2);
    assert!(overview.summary.contains("Executive Factual Overview"));
}

#[test]
fn test_aged_session_detection() {
    let compressor = SemanticCompressor::with_config(SemanticCompressorConfig {
        aged_session_hours: 24,
        ..SemanticCompressorConfig::default()
    });

    let recent_turn = DialogueTurn::new("t1", "recent-sess", "user", "Recent message", 0)
        .with_timestamp(Utc::now());
    assert!(!compressor.is_session_aged(&[recent_turn]));

    let old_timestamp = Utc::now() - Duration::hours(48);
    let aged_turn = DialogueTurn::new("t2", "aged-sess", "user", "Old message", 0)
        .with_timestamp(old_timestamp);
    assert!(compressor.is_session_aged(&[aged_turn]));
}
