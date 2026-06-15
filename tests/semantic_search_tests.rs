use std::sync::Arc;
use xavier::embedding::pipeline::LocalEmbeddingPipeline;
use xavier::embedding::{Embedder, EmbeddingError};
use xavier::memory::schema::ClearanceLevel;
use xavier::memory::store::{InMemoryMemoryStore, MemoryRecord, MemoryStore};

#[derive(Debug, Clone)]
struct TestEmbedder {
    dimension: usize,
}

impl TestEmbedder {
    fn new(dimension: usize) -> Self {
        Self { dimension }
    }
}

#[async_trait::async_trait]
impl Embedder for TestEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        use sha2::{Digest, Sha256};

        let mut vector = vec![0.0; self.dimension];
        for token in text
            .split(|character: char| !character.is_ascii_alphanumeric())
            .map(str::to_ascii_lowercase)
            .filter(|token| !token.is_empty())
        {
            let mut hasher = Sha256::new();
            hasher.update(token.as_bytes());
            let hash = hasher.finalize();
            let index =
                u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]) as usize % self.dimension;
            vector[index] += 1.0;
        }

        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut vector {
                *value /= norm;
            }
        }

        Ok(vector)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[tokio::test]
async fn test_local_embedding_pipeline_authorization() {
    let workspace_id = "test-workspace";
    let store = Arc::new(InMemoryMemoryStore::new());
    let embedder = Arc::new(TestEmbedder::new(384));

    // 1. Authorized record
    let authorized = MemoryRecord {
        id: "auth-1".into(),
        workspace_id: workspace_id.into(),
        path: "auth/1".into(),
        content: "Authorized content".into(),
        clearance: ClearanceLevel::Unclassified,
        ..Default::default()
    };
    store.put(authorized).await.unwrap();

    // 2. Private record (visibility)
    let private_visibility = MemoryRecord {
        id: "private-1".into(),
        workspace_id: workspace_id.into(),
        path: "private/1".into(),
        content: "Private content".into(),
        metadata: serde_json::json!({"visibility": "private"}),
        ..Default::default()
    };
    store.put(private_visibility).await.unwrap();

    // 3. Private record (is_private)
    let private_flag = MemoryRecord {
        id: "private-2".into(),
        workspace_id: workspace_id.into(),
        path: "private/2".into(),
        content: "Private content 2".into(),
        metadata: serde_json::json!({"is_private": true}),
        ..Default::default()
    };
    store.put(private_flag).await.unwrap();

    // 4. Revoked record
    let revoked = MemoryRecord {
        id: "revoked-1".into(),
        workspace_id: workspace_id.into(),
        path: "revoked/1".into(),
        content: "Revoked content".into(),
        metadata: serde_json::json!({"revoked": true}),
        ..Default::default()
    };
    store.put(revoked).await.unwrap();

    // 5. Over-depth record (TopSecret)
    let over_depth = MemoryRecord {
        id: "over-depth-1".into(),
        workspace_id: workspace_id.into(),
        path: "over/1".into(),
        content: "Top secret content".into(),
        clearance: ClearanceLevel::TopSecret,
        ..Default::default()
    };
    store.put(over_depth).await.unwrap();

    let pipeline =
        LocalEmbeddingPipeline::with_consent(embedder, store.clone(), ClearanceLevel::Secret, true);
    let processed = pipeline.process_workspace(workspace_id).await.unwrap();

    assert_eq!(processed, 1, "Only one record should have been processed");

    // Verify embeddings
    let r1 = store.get(workspace_id, "auth-1").await.unwrap().unwrap();
    assert!(
        !r1.embedding.is_empty(),
        "Authorized record should have an embedding"
    );

    let r2 = store.get(workspace_id, "private-1").await.unwrap().unwrap();
    assert!(
        r2.embedding.is_empty(),
        "Private record should not have an embedding"
    );

    let r3 = store.get(workspace_id, "private-2").await.unwrap().unwrap();
    assert!(
        r3.embedding.is_empty(),
        "Private record should not have an embedding"
    );

    let r4 = store.get(workspace_id, "revoked-1").await.unwrap().unwrap();
    assert!(
        r4.embedding.is_empty(),
        "Revoked record should not have an embedding"
    );

    let r5 = store
        .get(workspace_id, "over-depth-1")
        .await
        .unwrap()
        .unwrap();
    assert!(
        r5.embedding.is_empty(),
        "Over-depth record should not have an embedding"
    );
}

#[tokio::test]
async fn test_local_embedding_pipeline_no_consent() {
    let workspace_id = "test-workspace-no-consent";
    let store = Arc::new(InMemoryMemoryStore::new());
    let embedder = Arc::new(TestEmbedder::new(384));

    let authorized = MemoryRecord {
        id: "auth-1".into(),
        workspace_id: workspace_id.into(),
        path: "auth/1".into(),
        content: "Authorized content".into(),
        clearance: ClearanceLevel::Unclassified,
        ..Default::default()
    };
    store.put(authorized).await.unwrap();

    let pipeline = LocalEmbeddingPipeline::with_consent(
        embedder,
        store.clone(),
        ClearanceLevel::Secret,
        false,
    );
    let processed = pipeline.process_workspace(workspace_id).await.unwrap();

    assert_eq!(
        processed, 0,
        "No records should be processed without consent"
    );

    let r1 = store.get(workspace_id, "auth-1").await.unwrap().unwrap();
    assert!(r1.embedding.is_empty());
}

#[tokio::test]
async fn test_semantic_search_integration() {
    let workspace_id = "semantic-test";
    let store = Arc::new(InMemoryMemoryStore::new());
    let embedder = Arc::new(TestEmbedder::new(384));

    let records = vec![
        (
            "rust",
            "Rust is a systems programming language focusing on safety.",
        ),
        (
            "python",
            "Python is an interpreted, high-level, general-purpose programming language.",
        ),
        (
            "coffee",
            "Coffee is a brewed drink prepared from roasted coffee beans.",
        ),
    ];

    for (id, content) in records {
        store
            .put(MemoryRecord {
                id: id.into(),
                workspace_id: workspace_id.into(),
                path: format!("doc/{}", id),
                content: content.into(),
                clearance: ClearanceLevel::Unclassified,
                ..Default::default()
            })
            .await
            .unwrap();
    }

    let pipeline = LocalEmbeddingPipeline::with_consent(
        embedder.clone(),
        store.clone(),
        ClearanceLevel::Secret,
        true,
    );
    pipeline.process_workspace(workspace_id).await.unwrap();

    // Verify search works (using MockEmbedder to get query vector)
    let query = "systems programming language";
    let query_vector = embedder.encode(query).await.unwrap();

    let all_records = store.list(workspace_id).await.unwrap();

    // Manual similarity search since InMemoryMemoryStore doesn't support vector search directly
    let mut scored: Vec<(f32, &MemoryRecord)> = all_records
        .iter()
        .map(|r| {
            let score = cosine_similarity(&query_vector, &r.embedding);
            (score, r)
        })
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    assert_eq!(scored[0].1.id, "rust");
}

fn cosine_similarity(v1: &[f32], v2: &[f32]) -> f32 {
    if v1.len() != v2.len() || v1.is_empty() {
        return 0.0;
    }
    let dot: f32 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
    let norm1: f32 = v1.iter().map(|a| a * a).sum::<f32>().sqrt();
    let norm2: f32 = v2.iter().map(|a| a * a).sum::<f32>().sqrt();
    if norm1 == 0.0 || norm2 == 0.0 {
        return 0.0;
    }
    dot / (norm1 * norm2)
}
