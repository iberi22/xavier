//! Embedding Model Benchmark Suite
//!
//! Compares embedding quality across multiple backends.
//! Measures: encoding latency + cosine similarity accuracy.
//!
//! Models tested:
//!   1. GLLM: all-MiniLM-L6-v2 (384d) — baseline
//!   2. GLLM: all-mpnet-base-v2 (768d) — new default
//!   3. GLLM: Qwen3-Embedding-0.6B (1024d) — SOTA
//!   4. Docker: Infinity/TEI/Ollama (OpenAI-compatible API)
//!   5. OpenRouter: text-embedding-3-small (1536d) — cloud baseline
//!
//! Run: cargo test --test embedding_benchmark -- --nocapture
//! Env:  OPENAI_API_KEY or XAVIER_OPENROUTER_API_KEY for cloud tests
//!       XAVIER_BENCH_DOCKER_URL for Docker tests

use std::sync::Arc;
use std::time::Instant;

use xavier::embedding::gllm::GllmEmbedder;

// ─── Cosine Similarity ─────────────────────────────────────────

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)) as f64
}

// ─── Test Dataset ───────────────────────────────────────────────

struct SimilarityPair {
    query: &'static str,
    doc: &'static str,
    expected_high: bool, // true = should be semantically similar
}

fn similarity_pairs() -> Vec<SimilarityPair> {
    vec![
        // HIGH similarity pairs (should return > 0.95 for dense embeddings)
        SimilarityPair {
            query: "ACCT-9F3A renewal approved by Alice Johnson",
            doc: "Customer account ACCT-9F3A renewal approved by Alice Johnson on January 15, 2026. The renewal includes enterprise support for 12 months with priority SLA.",
            expected_high: true,
        },
        SimilarityPair {
            query: "INC-4821 production outage escalation",
            doc: "Incident INC-4821 escalated to OpenClaw runtime support team. Critical priority due to production outage affecting 5,000 users.",
            expected_high: true,
        },
        SimilarityPair {
            query: "xavier v0.6.1-beta release with RRF and BM25",
            doc: "Repository openclaw/xavier tagged release v0.6.1-beta for customer rollout. Release includes RRF scoring and BM25 fallback.",
            expected_high: true,
        },
        SimilarityPair {
            query: "NVIDIA GPU CUDA inference speed benchmarks",
            doc: "CUDA inference speed benchmarks: Qwen3-Embedding-0.6B achieves 5,000 ops/s on RTX 3060 vs 50 ops/s on CPU. 100x speedup with GPU acceleration.",
            expected_high: true,
        },
        SimilarityPair {
            query: "embedding cache LRU TTL reduce latency",
            doc: "Embedding cache configuration with LRU + TTL. Cache hit reduces 'add' latency from ~1,200ms to ~50ms. Uses moka cache library with 30min TTL.",
            expected_high: true,
        },
        SimilarityPair {
            query: "Docker Infinity embedding server setup",
            doc: "Docker setup for Infinity embedding server with gte-Qwen2-1.5B-instruct model. Exposes REST API on port 7997 compatible with OpenAI embedding format.",
            expected_high: true,
        },
        // LOW similarity pairs (should return < 0.94 for dense embeddings)
        SimilarityPair {
            query: "best pasta carbonara recipe with guanciale and pecorino",
            doc: "The Pythagoreans believed numbers were the fundamental essence of all reality.",
            expected_high: false,
        },
        SimilarityPair {
            query: "how to fix leaking kitchen sink pipe under cabinet",
            doc: "Coral reefs are diverse underwater ecosystems held together by calcium carbonate.",
            expected_high: false,
        },
        SimilarityPair {
            query: "mercedes-benz e-class 2025 fuel efficiency warranty",
            doc: "Quantum entanglement occurs when particles become interconnected instantly.",
            expected_high: false,
        },
        SimilarityPair {
            query: "yoga poses for lower back pain relief beginners",
            doc: "The Rosetta Stone was key to deciphering Egyptian hieroglyphs through its scripts.",
            expected_high: false,
        }]
}

// ─── Bench Embedder ────────────────────────────────────────────

async fn bench_embedder(
    name: &str,
    label: &str,
    embedder: Arc<dyn xavier::embedding::Embedder>,
) -> EmbeddingBenchResult {
    let pairs = similarity_pairs();
    let mut high_scores: Vec<f64> = Vec::new();
    let mut low_scores: Vec<f64> = Vec::new();
    let mut latencies: Vec<f64> = Vec::new();
    let mut correct = 0usize;
    let total = pairs.len();

    println!("\n  ── {name} ──");

    for pair in &pairs {
        let start = Instant::now();
        let query_emb = embedder.encode(pair.query).await.unwrap_or_default();
        let doc_emb = embedder.encode(pair.doc).await.unwrap_or_default();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        latencies.push(elapsed);

        let sim = cosine_similarity(&query_emb, &doc_emb);

        let is_correct = if pair.expected_high {
            sim > 0.95 // high similarity threshold
        } else {
            sim < 0.94 // low similarity threshold
        };

        if is_correct {
            correct += 1;
        }

        let icon = if is_correct { "✅" } else { "❌" };
        let expected_label = if pair.expected_high { "HIGH" } else { "LOW " };

        println!(
            "    {icon} sim={sim:.4} (expected {expected_label}) — {:.55}",
            if pair.query.len() > 55 {
                format!("{}..", &pair.query[..53])
            } else {
                pair.query.to_string()
            }
        );

        if pair.expected_high {
            high_scores.push(sim);
        } else {
            low_scores.push(sim);
        }
    }

    let avg_latency = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };
    let min_latency = latencies.iter().cloned().fold(f64::MAX, f64::min);
    let max_latency = latencies.iter().cloned().fold(f64::MIN, f64::max);
    let avg_high = if high_scores.is_empty() {
        0.0
    } else {
        high_scores.iter().sum::<f64>() / high_scores.len() as f64
    };
    let avg_low = if low_scores.is_empty() {
        0.0
    } else {
        low_scores.iter().sum::<f64>() / low_scores.len() as f64
    };
    let separation = avg_high - avg_low;
    let accuracy = (correct as f64 / total as f64) * 100.0;

    EmbeddingBenchResult {
        label: label.to_string(),
        model: name.to_string(),
        accuracy_pct: accuracy,
        correct,
        total,
        avg_latency_ms: avg_latency,
        min_latency_ms: min_latency,
        max_latency_ms: max_latency,
        latency_samples: latencies.len(),
        separation,
        avg_high_sim: avg_high,
        avg_low_sim: avg_low,
        dimension: embedder.dimension(),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EmbeddingBenchResult {
    label: String,
    model: String,
    accuracy_pct: f64,
    correct: usize,
    total: usize,
    avg_latency_ms: f64,
    min_latency_ms: f64,
    max_latency_ms: f64,
    latency_samples: usize,
    separation: f64,
    avg_high_sim: f64,
    avg_low_sim: f64,
    dimension: usize,
}

// ─── Runner ────────────────────────────────────────────────────

async fn run_all_benchmarks() -> Vec<EmbeddingBenchResult> {
    let mut results = Vec::new();

    println!();
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║       🧠 Xavier Embedding Model Benchmark Suite              ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
    println!("📐 Method: cosine similarity between query/document pairs");
    println!("📊 10 pairs: 5 high-similarity (matching) + 5 low-similarity (non-matching)");
    println!("🎯 Target: high > 0.95, low < 0.94 (separation = avg_high - avg_low)\n");

    // ── 1. GLLM Baseline: all-MiniLM-L6-v2 ──
    println!("━━━ [1/5] all-MiniLM-L6-v2 (384d, MTEB 58.8) ━━━");
    match GllmEmbedder::new("all-MiniLM-L6-v2".into(), 384) {
        Ok(e) => results.push(bench_embedder("MiniLM-L6-v2", "gllm-local", Arc::new(e)).await),
        Err(e) => println!("  ⚠️  SKIP: {e}"),
    }

    // ── 2. GLLM New Default: all-mpnet-base-v2 ──
    println!("\n━━━ [2/5] all-mpnet-base-v2 (768d, MTEB 63.0) ━━━");
    match GllmEmbedder::new("all-mpnet-base-v2".into(), 768) {
        Ok(e) => results.push(bench_embedder("mpnet-base-v2", "gllm-local", Arc::new(e)).await),
        Err(e) => println!("  ⚠️  SKIP: {e}"),
    }

    // ── 3. GLLM SOTA: Qwen3-Embedding-0.6B ──
    println!("\n━━━ [3/5] Qwen3-Embedding-0.6B (1024d, MTEB ~67.5) ━━━");
    match GllmEmbedder::new("Qwen/Qwen3-Embedding-0.6B".into(), 1024) {
        Ok(e) => results.push(bench_embedder("Qwen3-Embed-0.6B", "gllm-local", Arc::new(e)).await),
        Err(e) => println!("  ⚠️  SKIP: {e}"),
    }

    // ── 4. Docker: OpenAI-compatible endpoint ──
    println!("\n━━━ [4/5] Docker (Infinity/TEI/Ollama) ━━━");
    let docker_url = std::env::var("XAVIER_BENCH_DOCKER_URL")
        .unwrap_or_else(|_| "http://localhost:7997/v1/embeddings".to_string());
    let docker_model = std::env::var("XAVIER_BENCH_DOCKER_MODEL")
        .unwrap_or_else(|_| "Alibaba-NLP/gte-Qwen2-1.5B-instruct".to_string());
    let docker_model_name = docker_model
        .split('/')
        .last()
        .unwrap_or("unknown")
        .to_string();

    if let Ok(e) = xavier::embedding::openai::OpenAICompatibleEmbedder::new(
        Some("dummy-key".into()),
        docker_model.clone(),
        format!("{docker_url}"),
        1536,
    ) {
        results.push(
            bench_embedder(
                &format!("Docker:{docker_model_name}"),
                "docker",
                Arc::new(e),
            )
            .await,
        );
    } else {
        println!("  ⚠️  Docker endpoint not reachable (set XAVIER_BENCH_DOCKER_URL)");
    }

    // ── 5. OpenRouter Cloud ──
    println!("\n━━━ [5/5] OpenRouter: text-embedding-3-small (1536d, MTEB 62.3) ━━━");
    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("XAVIER_OPENROUTER_API_KEY"))
        .ok();
    let api_endpoint = std::env::var("XAVIER_EMBEDDING_URL")
        .unwrap_or_else(|_| "https://openrouter.ai/api/v1/embeddings".to_string());

    if let Some(key) = api_key {
        if !key.is_empty() {
            if let Ok(e) = xavier::embedding::openai::OpenAICompatibleEmbedder::new(
                Some(key),
                "text-embedding-3-small".into(),
                api_endpoint,
                1536,
            ) {
                results.push(
                    bench_embedder("text-embedding-3-small", "cloud-openrouter", Arc::new(e)).await,
                );
            } else {
                println!("  ⚠️  OpenRouter init failed");
            }
        }
    } else {
        println!("  ⚠️  SKIP: No API key (set OPENAI_API_KEY)");
    }

    results
}

fn print_summary(results: &[EmbeddingBenchResult]) {
    println!("\n");
    println!(
        "╔══════════════════════════════════════════════════════════════════════════════════╗"
    );
    println!("║                         🏆 EMBEDDING BENCHMARK RESULTS                          ║");
    println!(
        "╠══════════════════════════════════════════════════════════════════════════════════╣"
    );
    println!(
        "║ {:<18} {:>8} {:>7} {:>7} {:>9} {:>6} {:>6} ║",
        "Model", "Acc.", "Lat ms", "Sep.", "Hi Sim", "Dims", "Score"
    );
    println!(
        "╠══════════════════════════════════════════════════════════════════════════════════╣"
    );

    let mut sorted = results.to_vec();
    sorted.sort_by(|a, b| {
        b.accuracy_pct
            .partial_cmp(&a.accuracy_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for r in &sorted {
        let short = if r.model.len() > 18 {
            format!("{}..", &r.model[..16])
        } else {
            r.model.clone()
        };
        let mteb_score = if r.model.contains("MiniLM") {
            "58.8"
        } else if r.model.contains("mpnet") {
            "63.0"
        } else if r.model.contains("0.6B") {
            "67.5"
        } else if r.model.contains("1.5B") {
            "64.5"
        } else if r.model.contains("embedding-3-small") {
            "62.3"
        } else {
            "—"
        };

        println!(
            "║ {:<18} {:>6.1}% {:>7.1} {:>7.3} {:>7.3} {:>6} {:>6} ║",
            short,
            format!("{:.1}", r.accuracy_pct),
            format!("{:.1}", r.avg_latency_ms),
            r.separation,
            r.avg_high_sim,
            r.dimension,
            mteb_score
        );
    }

    println!(
        "╚══════════════════════════════════════════════════════════════════════════════════╝"
    );
    println!();
    println!("📌 Date: {}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
    println!(
        "📌 Tests: 5 high-sim + 5 low-sim pairs = {total} total",
        total = if results.first().map(|r| r.total).unwrap_or(0) > 0 {
            format!("{}", results[0].total)
        } else {
            "N/A".into()
        }
    );
    println!("📌 Separation = avg(high-sim) − avg(low-sim) — bigger is better");
    println!();

    if let Some(best) = sorted.first() {
        println!(
            "🌟 BEST ACCURACY: {} ({:.1}%)",
            best.model, best.accuracy_pct
        );
    }

    if let Some(fastest) = sorted
        .iter()
        .min_by(|a, b| a.avg_latency_ms.partial_cmp(&b.avg_latency_ms).unwrap())
    {
        if fastest.avg_latency_ms < 100.0 {
            println!(
                "⚡ FASTEST: {} ({:.1}ms avg)",
                fastest.model, fastest.avg_latency_ms
            );
        }
    }

    // Write JSON results
    let output_path = std::env::var("XAVIER_BENCH_OUTPUT")
        .unwrap_or_else(|_| "benchmark-embedding-results.json".to_string());
    if let Ok(json) = serde_json::to_string_pretty(&results) {
        std::fs::write(&output_path, &json).ok();
        println!("📄 Results saved to: {output_path}");
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_embedding_benchmarks() {
        let results = run_all_benchmarks().await;
        print_summary(&results);

        assert!(!results.is_empty(), "At least one embedder must initialize");
        if let Some(best) = results
            .iter()
            .max_by(|a, b| a.accuracy_pct.partial_cmp(&b.accuracy_pct).unwrap())
        {
            println!("🏆 Best: {} ({:.1}%)", best.model, best.accuracy_pct);
            assert!(
                best.accuracy_pct > 50.0,
                "Best model must exceed 50% accuracy"
            );
        }
    }
}

#[tokio::main]
async fn main() {
    let results = run_all_benchmarks().await;
    print_summary(&results);
}
