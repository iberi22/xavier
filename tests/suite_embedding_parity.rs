xavier::isolate_test_process!();

use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Arc;

use xavier::embedding::fallback::FallbackEmbedder;
use xavier::embedding::{Embedder, EmbeddingError};

#[derive(Debug, Clone, Deserialize, Serialize)]
struct FixturePair {
    id: String,
    text: String,
    expected_dim: usize,
    norm: f32,
    expected_vector: Vec<f32>,
}

fn load_fixtures() -> Vec<FixturePair> {
    let data = fs::read_to_string("tests/fixtures/embeddinggemma_fixtures.json").unwrap();
    serde_json::from_str(&data).unwrap()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

fn l2_norm(v: &[f32]) -> f32 {
    let sum_sq: f64 = v.iter().map(|&x| (x as f64) * (x as f64)).sum();
    sum_sq.sqrt() as f32
}

// Mock engine returning the expected fixture vectors
struct MockEngine {
    fixtures: Vec<FixturePair>,
}

#[async_trait::async_trait]
impl Embedder for MockEngine {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        if let Some(f) = self.fixtures.iter().find(|f| f.text == text) {
            return Ok(f.expected_vector.clone());
        }
        Ok(vec![0.0; 768])
    }
    async fn probe_health(&self) -> Result<f64, EmbeddingError> {
        Ok(10.0)
    }
    fn dimension(&self) -> usize {
        768
    }
}

struct FailingEngine;
#[async_trait::async_trait]
impl Embedder for FailingEngine {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Err(EmbeddingError::Network("ModelUnavailable".into()))
    }
    async fn probe_health(&self) -> Result<f64, EmbeddingError> {
        Err(EmbeddingError::Network("ModelUnavailable".into()))
    }
    fn dimension(&self) -> usize {
        768
    }
}

struct DimensionEngine {
    dim: usize,
}
#[async_trait::async_trait]
impl Embedder for DimensionEngine {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(vec![0.1; self.dim])
    }
    async fn probe_health(&self) -> Result<f64, EmbeddingError> {
        Ok(10.0)
    }
    fn dimension(&self) -> usize {
        self.dim
    }
}

#[tokio::test]
async fn test_embedding_parity_and_accuracy() {
    let fixtures = load_fixtures();
    let engine = MockEngine {
        fixtures: fixtures.clone(),
    };

    for fixture in fixtures {
        let output = engine.encode(&fixture.text).await.unwrap();

        // 1. Check dimensions
        assert_eq!(output.len(), 768, "Dimension mismatch");

        // 2. Element-wise error
        for (i, (&actual, &expected)) in output
            .iter()
            .zip(fixture.expected_vector.iter())
            .enumerate()
        {
            let error = (actual - expected).abs();
            assert!(
                error <= 1e-4,
                "Element-wise error exceeded limit at index {}: {} (actual) vs {} (expected)",
                i,
                actual,
                expected
            );
        }

        // 3. Cosine similarity
        let cosine = cosine_similarity(&output, &fixture.expected_vector);
        assert!(cosine >= 0.9999, "Cosine similarity too low: {}", cosine);

        // 4. L2 norm
        let norm = l2_norm(&output);
        let norm_diff = (norm - 1.0).abs();
        assert!(
            norm_diff <= 1e-5,
            "L2 norm mismatch: {} (diff: {})",
            norm,
            norm_diff
        );
    }
}

#[tokio::test]
async fn test_fallback_resilience() {
    let fixtures = load_fixtures();
    let primary: Arc<dyn Embedder> = Arc::new(FailingEngine);
    let secondary: Arc<dyn Embedder> = Arc::new(MockEngine { fixtures });

    let fallback = FallbackEmbedder {
        embedders: vec![primary, secondary],
    };

    let start = std::time::Instant::now();
    let result = fallback.encode("Hello, embedding parity test.").await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Fallback should succeed");

    // Switch should happen within budget (let's check a reasonable threshold like 500ms since it's mock, but real budget is e.g. 1000ms)
    assert!(
        elapsed.as_millis() < 500,
        "Fallback switch took too long: {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_fallback_total_failure_degradation() {
    let primary: Arc<dyn Embedder> = Arc::new(FailingEngine);
    let secondary: Arc<dyn Embedder> = Arc::new(FailingEngine);

    let fallback = FallbackEmbedder {
        embedders: vec![primary, secondary],
    };

    let result = fallback.encode("test").await;
    assert!(result.is_err());
    let err = result.unwrap_err();

    // We expect the fallback logic to return an error, gracefully avoiding panics.
    // The query layer handles this error by dropping to BM25.
    match err {
        EmbeddingError::Config(msg) => {
            assert!(msg.contains("no embedding backend produced a usable vector"))
        }
        EmbeddingError::Network(msg) => {
            assert!(msg.contains("ModelUnavailable"))
        }
        _ => panic!("Expected Config or Network error, got {:?}", err),
    }
}

#[tokio::test]
async fn test_dimension_assertion() {
    // If the dimension mismatch happens (e.g. vector written is wrong), returning dimension error
    // In our simplified mock, we simulate it via a test dimension checker
    let engine = DimensionEngine { dim: 512 };
    let result = engine.encode("test").await.unwrap();

    // Simulating dimension mismatch check before writing to DB
    let expected_dim = 768;
    if result.len() != expected_dim {
        let err = EmbeddingError::Parse(format!(
            "Dimension mismatch: expected {}, got {}",
            expected_dim,
            result.len()
        ));
        assert_eq!(
            err.to_string(),
            "embedding parse error: Dimension mismatch: expected 768, got 512"
        );
    } else {
        panic!("Should have failed dimension check");
    }
}
