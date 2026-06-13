//! Quick Embedding Smoke Test - verifica que gllm cargue y encodee
//!
//! Run: cargo test --test quick_embed_test --features local-gllm -- --nocapture
//! Env: XAVIER_GLLM_MODEL=all-MiniLM-L6-v2 (or mpnet, Qwen3-Embedding)

use std::time::Instant;
use xavier::embedding::Embedder;

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)) as f64
}

#[tokio::test]
async fn test_embedding_models() {
    let model =
        std::env::var("XAVIER_GLLM_MODEL").unwrap_or_else(|_| "all-MiniLM-L6-v2".to_string());
    let dim = if model.contains("MiniLM") {
        384
    } else if model.contains("mpnet") {
        768
    } else {
        1024
    };

    println!("\n🔬 Testing: {model} ({dim}d)");

    let embedder = xavier::embedding::gllm::GllmEmbedder::new(model.clone(), dim)
        .expect("Failed to load embedder");

    let queries = vec![
        ("HIGH-1", "ACCT-9F3A renewal approved by Alice Johnson",
         "Customer account ACCT-9F3A renewal approved by Alice Johnson on January 15."),
        ("HIGH-2", "INC-4821 production outage escalation",
         "Incident INC-4821 escalated to OpenClaw runtime support team due to production outage."),
        ("HIGH-3", "xavier v0.6.1-beta release with RRF and BM25",
         "Repository openclaw/xavier tagged release v0.6.1-beta for customer rollout with RRF."),
        ("LOW-1",  "ACCT-9F3A renewal",
         "CUDA inference speed benchmarks for GPU acceleration of embedding models."),
        ("LOW-2",  "GPU inference benchmarks",
         "Customer account ACCT-9F3A renewal approved by Alice Johnson."),
    ];

    let mut total_latency = 0.0f64;
    let mut correct = 0u32;
    let mut high_sims = Vec::new();
    let mut low_sims = Vec::new();

    for (label, query, doc) in &queries {
        let start = Instant::now();
        let q_emb = embedder.encode(query).await.unwrap();
        let d_emb = embedder.encode(doc).await.unwrap();
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        total_latency += elapsed;

        let sim = cosine_similarity(&q_emb, &d_emb);
        let is_high = label.starts_with("HIGH");
        let pass = if is_high { sim > 0.4 } else { sim < 0.5 };

        if pass {
            correct += 1;
        }
        if is_high {
            high_sims.push(sim);
        } else {
            low_sims.push(sim);
        }

        println!(
            "  {} {label:6} sim={sim:.4} ({:.1}ms) {}",
            if pass { "✅" } else { "❌" },
            elapsed,
            query
        );
    }

    let avg_latency = total_latency / queries.len() as f64;
    let avg_high = if high_sims.is_empty() {
        0.0
    } else {
        high_sims.iter().sum::<f64>() / high_sims.len() as f64
    };
    let avg_low = if low_sims.is_empty() {
        0.0
    } else {
        low_sims.iter().sum::<f64>() / low_sims.len() as f64
    };
    let separation = avg_high - avg_low;
    let accuracy = (correct as f64 / queries.len() as f64) * 100.0;

    println!();
    println!("📊 Results for {model}:");
    println!(
        "   Accuracy:     {:.0}% ({correct}/{})",
        accuracy,
        queries.len()
    );
    println!("   Avg Latency:  {:.1}ms", avg_latency);
    println!("   Avg HIGH sim: {:.4}", avg_high);
    println!("   Avg LOW sim:  {:.4}", avg_low);
    println!("   Separation:   {:.4}", separation);
    println!("   Dimensions:   {dim}");

    assert!(
        accuracy >= 60.0,
        "Model {model} accuracy too low: {accuracy}%"
    );
}
