use criterion::{black_box, criterion_group, criterion_main, Criterion};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use tokio::runtime::Runtime;
use xavier::domain::memory::belief::BeliefEdge;
use xavier::memory::sqlite_vec_store::{VecSqliteMemoryStore, VecSqliteStoreConfig};
use xavier::memory::{MemoryRecord, MemoryStore};

fn stable_key(kind: &str, parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    digest.update(kind.as_bytes());
    for part in parts {
        digest.update([0u8]);
        digest.update(part.as_bytes());
    }
    xavier::crypto::hex_encode(digest.finalize())
}

fn bench_hybrid_search(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let temp_dir = tempdir().expect("temp dir");
    let workspace_id = "bench-hybrid";

    let store = runtime.block_on(async {
        VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: temp_dir.path().join("hybrid-bench.db"),
            embedding_dimensions: 3,
        })
        .await
        .expect("vec store")
    });

    runtime.block_on(async {
        let docs = [
            (
                "memory/account-renewal",
                "Customer account ACCT-9F3A renewal approved by Alice Johnson.",
                vec![0.0, 1.0, 0.0],
            ),
            (
                "memory/account-summary",
                "Enterprise renewal planning notes for the customer account.",
                vec![1.0, 0.0, 0.0],
            ),
            (
                "memory/incident",
                "Incident INC-4821 escalated to OpenClaw runtime support.",
                vec![0.0, 0.0, 1.0],
            ),
            (
                "memory/runtime-notes",
                "Runtime support queue for infrastructure incidents and pager load.",
                vec![1.0, 0.0, 0.0],
            ),
            (
                "memory/repo-release",
                "Repository openclaw/xavier tagged release v0.4.1 for customer rollout.",
                vec![0.0, 1.0, 1.0],
            ),
            (
                "memory/release-summary",
                "Release planning notes for the next customer rollout.",
                vec![1.0, 1.0, 0.0],
            ),
        ];

        for (path, content, embedding) in docs {
            store
                .put(MemoryRecord {
                    id: stable_key("memory", &[workspace_id, path]),
                    workspace_id: workspace_id.to_string(),
                    path: path.to_string(),
                    content: content.to_string(),
                    metadata: serde_json::json!({}),
                    embedding,
                    score: 0.0,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    revision: 1,
                    primary: true,
                    deleted_at: None,
                    parent_id: None,
                    cluster_id: None,
                    level: Default::default(),
                    relation: None,
                    clearance: Default::default(),
                    revisions: Vec::new(),
                    content_iv: None,
                    encrypted_dek: None,
                    metadata_iv: None,
                })
                .await
                .expect("seed memory");
        }

        store
            .save_beliefs(
                workspace_id,
                vec![
                    BeliefEdge::new(
                        "ACCT-9F3A".to_string(),
                        "Alice Johnson".to_string(),
                        "approved_by".to_string(),
                        0.9,
                        stable_key("memory", &[workspace_id, "memory/account-renewal"]),
                    ),
                    BeliefEdge::new(
                        "INC-4821".to_string(),
                        "OpenClaw".to_string(),
                        "handled_by".to_string(),
                        0.8,
                        stable_key("memory", &[workspace_id, "memory/incident"]),
                    ),
                ],
            )
            .await
            .expect("seed beliefs");
    });

    let cases = [
        (
            "ACCT-9F3A renewal",
            [1.0, 0.0, 0.0],
            "memory/account-renewal",
        ),
        ("INC-4821 OpenClaw", [1.0, 0.0, 0.0], "memory/incident"),
        (
            "openclaw/xavier v0.4.1",
            [1.0, 1.0, 0.0],
            "memory/repo-release",
        ),
    ];

    let (vector_hits, hybrid_hits) = runtime.block_on(async {
        let mut vector_hits = 0usize;
        let mut hybrid_hits = 0usize;

        for (query, embedding, expected_path) in &cases {
            let vector_results = store
                .hybrid_search_with_embedding(workspace_id, query, embedding.to_vec(), None, 3)
                .await
                .expect("vector results");
            if vector_results
                .first()
                .is_some_and(|result| result.record.path == *expected_path)
            {
                vector_hits += 1;
            }

            let hybrid_results = store
                .hybrid_search_with_embedding(workspace_id, query, embedding.to_vec(), None, 3)
                .await
                .expect("hybrid results");
            if hybrid_results
                .first()
                .is_some_and(|result| result.record.path == *expected_path)
            {
                hybrid_hits += 1;
            }
        }

        (vector_hits, hybrid_hits)
    });

    println!(
        "hybrid_search_hit_rate vector={}/{} hybrid={}/{}",
        vector_hits,
        cases.len(),
        hybrid_hits,
        cases.len()
    );

    c.bench_function("vector_search_exact_match_queries", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for (query, embedding, _) in &cases {
                    let q = black_box(*query);
                    let emb = black_box(embedding.to_vec());
                    let res = store
                        .hybrid_search_with_embedding(black_box(workspace_id), q, emb, None, 3)
                        .await;
                    black_box(res).ok();
                }
            })
        });
    });

    c.bench_function("hybrid_search_exact_match_queries", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for (query, embedding, _) in &cases {
                    let q = black_box(*query);
                    let emb = black_box(embedding.to_vec());
                    let res = store
                        .hybrid_search_with_embedding(black_box(workspace_id), q, emb, None, 3)
                        .await;
                    black_box(res).ok();
                }
            })
        });
    });
}

fn bench_memory_store_operations(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let temp_dir = tempdir().expect("temp dir");
    let workspace_id = "bench-mem-ops";

    let store = runtime.block_on(async {
        VecSqliteMemoryStore::new(VecSqliteStoreConfig {
            path: temp_dir.path().join("mem-ops-bench.db"),
            embedding_dimensions: 3,
        })
        .await
        .expect("vec store")
    });

    // 1. Benchmark Put operation
    c.bench_function("memory_store_put_record", |b| {
        let mut index = 0u64;
        b.iter(|| {
            index += 1;
            let path = format!("memory/bench/doc-{}", index);
            let id = stable_key("memory", &[workspace_id, &path]);
            let record = MemoryRecord {
                id,
                workspace_id: workspace_id.to_string(),
                path,
                content: format!(
                    "This is benchmark document number {} for standard put operations.",
                    index
                ),
                metadata: serde_json::json!({"index": index}),
                embedding: vec![1.0, 0.0, 0.0],
                score: 0.0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                revision: 1,
                primary: true,
                deleted_at: None,
                parent_id: None,
                cluster_id: None,
                level: Default::default(),
                relation: None,
                clearance: Default::default(),
                revisions: Vec::new(),
                content_iv: None,
                encrypted_dek: None,
                metadata_iv: None,
            };

            runtime.block_on(async {
                let res = store.put(black_box(record)).await;
                black_box(res).ok();
            });
        });
    });

    // 2. Benchmark Get operation
    // Seed a specific record to benchmark retrieval
    let target_id = stable_key("memory", &[workspace_id, "memory/bench/target-doc"]);
    runtime.block_on(async {
        store
            .put(MemoryRecord {
                id: target_id.clone(),
                workspace_id: workspace_id.to_string(),
                path: "memory/bench/target-doc".to_string(),
                content: "This is a target document for benchmarking retrieve and get operations."
                    .to_string(),
                metadata: serde_json::json!({}),
                embedding: vec![1.0, 0.0, 0.0],
                score: 0.0,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
                revision: 1,
                primary: true,
                deleted_at: None,
                parent_id: None,
                cluster_id: None,
                level: Default::default(),
                relation: None,
                clearance: Default::default(),
                revisions: Vec::new(),
                content_iv: None,
                encrypted_dek: None,
                metadata_iv: None,
            })
            .await
            .expect("seed target");
    });

    c.bench_function("memory_store_get_record", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let res = store
                    .get(black_box(workspace_id), black_box(&target_id))
                    .await;
                black_box(res).ok();
            });
        });
    });

    // 3. Benchmark Delete operation
    c.bench_function("memory_store_delete_record", |b| {
        let mut index = 0u64;
        b.iter(|| {
            index += 1;
            let path = format!("memory/bench/delete-{}", index);
            let id = stable_key("memory", &[workspace_id, &path]);

            // Setup the record first
            runtime.block_on(async {
                let record = MemoryRecord {
                    id: id.clone(),
                    workspace_id: workspace_id.to_string(),
                    path: path.clone(),
                    content: "Temporary doc for deletion benchmark".to_string(),
                    metadata: serde_json::json!({}),
                    embedding: vec![1.0, 0.0, 0.0],
                    score: 0.0,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                    revision: 1,
                    primary: true,
                    deleted_at: None,
                    parent_id: None,
                    cluster_id: None,
                    level: Default::default(),
                    relation: None,
                    clearance: Default::default(),
                    revisions: Vec::new(),
                    content_iv: None,
                    encrypted_dek: None,
                    metadata_iv: None,
                };
                store.put(record).await.unwrap();

                // Now benchmark delete
                let res = store.delete(black_box(workspace_id), black_box(&id)).await;
                black_box(res).ok();
            });
        });
    });
}

criterion_group!(
    hybrid_search_benches,
    bench_hybrid_search,
    bench_memory_store_operations
);
criterion_main!(hybrid_search_benches);
