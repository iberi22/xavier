//! Criterion latency benchmarks for Xavier's embedding pipeline.
//!
//! Benchmark scenarios:
//! 1. Cold Ollama round-trip (env-gated via `XAVIER_BENCH_OLLAMA_URL`, graceful skip offline).
//! 2. Warm cache hit via `EmbeddingCache::get_or_embed`.
//! 3. Batch of 100 `content_hash` + cache inserts.
//! 4. Single OpenAI call (env-gated via `XAVIER_BENCH_OPENAI_KEY`, graceful skip offline).
//! 5. `LocalEmbeddingPipeline::process_workspace` with mocked store.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tokio::runtime::Runtime;

use xavier::embedding::cache::{content_hash, EmbeddingCache, EmbeddingCacheConfig};
use xavier::embedding::ollama::OllamaEmbedder;
use xavier::embedding::openai::OpenAICompatibleEmbedder;
use xavier::embedding::pipeline::LocalEmbeddingPipeline;
use xavier::embedding::{Embedder, EmbeddingError};
use xavier::memory::schema::ClearanceLevel;
use xavier::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};

/// A mock embedder for deterministic local latency tests.
#[derive(Debug)]
struct MockEmbedder {
    dim: usize,
}

#[async_trait]
impl Embedder for MockEmbedder {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![0.01f32; self.dim])
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

/// Helper to report hot path performance thresholds (> 10 ms p50).
fn check_hot_path_threshold(name: &str, elapsed: Duration) {
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    if elapsed_ms > 10.0 {
        println!(
            "[HOT PATH WARNING] bench_function '{}' measured latency {:.3} ms (> 10 ms threshold)",
            name, elapsed_ms
        );
    } else {
        println!(
            "[PERF OK] bench_function '{}' measured latency {:.3} ms (<= 10 ms threshold)",
            name, elapsed_ms
        );
    }
}

/// Scenario 1: Cold Ollama round-trip (feature/env-gated, skip offline).
fn bench_cold_ollama_roundtrip(c: &mut Criterion) {
    let url = std::env::var("XAVIER_BENCH_OLLAMA_URL").ok();
    if let Some(endpoint) = url {
        if endpoint.trim().is_empty() {
            println!("Skipping cold_ollama_roundtrip: XAVIER_BENCH_OLLAMA_URL is empty");
            return;
        }
        let runtime = Runtime::new().expect("tokio runtime");
        let embedder = OllamaEmbedder::new(
            "nomic-embed-text".to_string(),
            endpoint,
            768,
            Duration::from_secs(5),
        )
        .expect("ollama embedder creation");

        let start = Instant::now();
        runtime.block_on(async {
            let _ = embedder.encode("cold ollama roundtrip test").await;
        });
        check_hot_path_threshold("cold_ollama_roundtrip", start.elapsed());

        c.bench_function("cold_ollama_roundtrip", |b| {
            b.iter(|| {
                runtime.block_on(async {
                    let res = embedder.encode(black_box("benchmark query text")).await;
                    black_box(res).ok();
                });
            });
        });
    } else {
        println!("Skipping cold_ollama_roundtrip: XAVIER_BENCH_OLLAMA_URL not set");
    }
}

/// Scenario 2: Warm cache hit via `EmbeddingCache::get_or_embed`.
fn bench_warm_cache_hit(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let config = EmbeddingCacheConfig {
        enabled: true,
        max_capacity: 10_000,
        ttl_hours: 24,
        db_path: PathBuf::from(":memory:"),
        persist: false,
        model_name: "default".to_string(),
    };
    let cache = EmbeddingCache::new(config);
    let mock_embedder = MockEmbedder { dim: 768 };
    let text = "warm cache hit test content for embedding benchmark";

    // Pre-fill cache instance
    runtime.block_on(async {
        cache
            .get_or_embed(&mock_embedder, text)
            .await
            .expect("pre-fill cache");
    });

    let start = Instant::now();
    runtime.block_on(async {
        let _ = cache.get_or_embed(&mock_embedder, text).await;
    });
    check_hot_path_threshold("warm_cache_hit", start.elapsed());

    c.bench_function("warm_cache_hit", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let res = cache
                    .get_or_embed(black_box(&mock_embedder), black_box(text))
                    .await;
                black_box(res).expect("warm cache hit");
            });
        });
    });
}

/// Scenario 3: Batch of 100 `content_hash` + cache inserts.
fn bench_batch_content_hash_cache_inserts(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");

    let start = Instant::now();
    runtime.block_on(async {
        let config = EmbeddingCacheConfig {
            enabled: true,
            max_capacity: 10_000,
            ttl_hours: 24,
            db_path: PathBuf::from(":memory:"),
            persist: false,
            model_name: "default".to_string(),
        };
        let cache = EmbeddingCache::new(config);
        let mock_embedder = MockEmbedder { dim: 768 };

        for i in 0..100 {
            let text = format!("batch item content index sample {}", i);
            let _hash = content_hash("default", &text);
            let _ = cache.get_or_embed(&mock_embedder, &text).await;
        }
    });
    check_hot_path_threshold("batch_100_content_hash_cache_inserts", start.elapsed());

    c.bench_function("batch_100_content_hash_cache_inserts", |b| {
        b.iter(|| {
            let config = EmbeddingCacheConfig {
                enabled: true,
                max_capacity: 10_000,
                ttl_hours: 24,
                db_path: PathBuf::from(":memory:"),
                persist: false,
                model_name: "default".to_string(),
            };
            let cache = EmbeddingCache::new(config);
            let mock_embedder = MockEmbedder { dim: 768 };

            runtime.block_on(async {
                for i in 0..100 {
                    let text = format!("batch item content index {}", i);
                    let _hash = content_hash("default", &text);
                    let res = cache.get_or_embed(&mock_embedder, &text).await;
                    black_box(res).ok();
                }
            });
        });
    });
}

/// Scenario 4: Single OpenAI call (env-gated via `XAVIER_BENCH_OPENAI_KEY`).
fn bench_single_openai_call(c: &mut Criterion) {
    let api_key = std::env::var("XAVIER_BENCH_OPENAI_KEY").ok();
    if let Some(key) = api_key {
        if key.trim().is_empty() {
            println!("Skipping single_openai_call: XAVIER_BENCH_OPENAI_KEY is empty");
            return;
        }
        let runtime = Runtime::new().expect("tokio runtime");
        let endpoint = std::env::var("XAVIER_BENCH_OPENAI_ENDPOINT")
            .unwrap_or_else(|_| "https://api.openai.com/v1/embeddings".to_string());
        let embedder = OpenAICompatibleEmbedder::new(
            Some(key),
            "text-embedding-3-small".to_string(),
            endpoint,
            1536,
            Duration::from_secs(10),
        )
        .expect("openai embedder creation");

        let start = Instant::now();
        runtime.block_on(async {
            let _ = embedder.encode("single openai call test").await;
        });
        check_hot_path_threshold("single_openai_call", start.elapsed());

        c.bench_function("single_openai_call", |b| {
            b.iter(|| {
                runtime.block_on(async {
                    let res = embedder.encode(black_box("benchmark query openai")).await;
                    black_box(res).ok();
                });
            });
        });
    } else {
        println!("Skipping single_openai_call: XAVIER_BENCH_OPENAI_KEY not set");
    }
}

/// Scenario 5: `LocalEmbeddingPipeline::process_workspace` with mocked store.
fn bench_pipeline_process_workspace(c: &mut Criterion) {
    let runtime = Runtime::new().expect("tokio runtime");
    let workspace_id = "bench-pipeline-ws";

    let start = Instant::now();
    runtime.block_on(async {
        let store = Arc::new(InMemoryMemoryStore::new());
        for i in 0..10 {
            let record = MemoryRecord {
                id: format!("doc-init-{}", i),
                workspace_id: workspace_id.to_string(),
                path: format!("memory/doc-init-{}", i),
                content: format!("Content for memory record document number {}", i),
                metadata: serde_json::json!({}),
                embedding: vec![],
                clearance: ClearanceLevel::Unclassified,
                ..Default::default()
            };
            store.put(record).await.expect("put record");
        }

        let embedder = Arc::new(MockEmbedder { dim: 768 });
        let pipeline =
            LocalEmbeddingPipeline::with_consent(embedder, store, ClearanceLevel::Secret, true);

        let _ = pipeline.process_workspace(workspace_id).await;
    });
    check_hot_path_threshold("pipeline_process_workspace_10_docs", start.elapsed());

    c.bench_function("pipeline_process_workspace_10_docs", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let store = Arc::new(InMemoryMemoryStore::new());
                for i in 0..10 {
                    let record = MemoryRecord {
                        id: format!("doc-{}", i),
                        workspace_id: workspace_id.to_string(),
                        path: format!("memory/doc-{}", i),
                        content: format!("Content for memory record document number {}", i),
                        metadata: serde_json::json!({}),
                        embedding: vec![],
                        clearance: ClearanceLevel::Unclassified,
                        ..Default::default()
                    };
                    store.put(record).await.expect("put record");
                }

                let embedder = Arc::new(MockEmbedder { dim: 768 });
                let pipeline = LocalEmbeddingPipeline::with_consent(
                    embedder,
                    store,
                    ClearanceLevel::Secret,
                    true,
                );

                let processed = pipeline
                    .process_workspace(black_box(workspace_id))
                    .await
                    .expect("process workspace");
                black_box(processed);
            });
        });
    });
}

criterion_group!(
    embedding_benches,
    bench_cold_ollama_roundtrip,
    bench_warm_cache_hit,
    bench_batch_content_hash_cache_inserts,
    bench_single_openai_call,
    bench_pipeline_process_workspace
);
criterion_main!(embedding_benches);
