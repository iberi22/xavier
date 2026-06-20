use criterion::{criterion_group, criterion_main, Criterion};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use xavier::retrieval::{AdaptiveGating, GatingConfig, NavigationPolicy};
use xavier::agents::hormer::Hormer;
use xavier::memory::qmd_memory::MemoryDocument;
use xavier::memory::entity_graph::EntityRecord;
use xavier::memory::entity_graph::EntityType;
use chrono::Utc;

fn bench_retrieval_hormer(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");

    // Setup mock data
    let working = vec![
        MemoryDocument {
            id: Some("doc1".to_string()),
            content: "Xavier is a cognitive memory system.".to_string(),
            ..MemoryDocument::default()
        }; 50
    ];
    let episodic = vec![];
    let semantic = vec![
        EntityRecord {
            id: "entity1".to_string(),
            name: "Xavier".to_string(),
            normalized_name: "xavier".to_string(),
            entity_type: EntityType::Concept,
            aliases: vec![],
            description: None,
            occurrence_count: 1,
            memory_count: 1,
            first_seen: Utc::now(),
            last_seen: Utc::now(),
            merged_from: vec![],
            trust_score: 0.5,
            trust_rank: 0,
        }; 50
    ];

    // Case 1: Standard Gating (without HORMER active policy)
    let gating_std = AdaptiveGating::new(GatingConfig::default());

    // Case 2: HORMER Gating
    let policy = Arc::new(RwLock::new(NavigationPolicy::with_defaults()));
    let _hormer = Hormer::new(Arc::clone(&policy));
    let gating_hormer = AdaptiveGating::with_policy(GatingConfig::default(), policy);

    c.bench_function("retrieval_standard", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = gating_std.retrieve(&working, &episodic, &semantic, "Xavier", None).await;
            });
        });
    });

    c.bench_function("retrieval_hormer", |b| {
        b.iter(|| {
            rt.block_on(async {
                let _ = gating_hormer.retrieve(&working, &episodic, &semantic, "Xavier", None).await;
            });
        });
    });
}

criterion_group!(retrieval_benches, bench_retrieval_hormer);
criterion_main!(retrieval_benches);
