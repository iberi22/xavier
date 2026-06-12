#[cfg(test)]
mod tests {
    use crate::agents::hormer::Hormer;
    use crate::retrieval::{LayerWeights, NavigationPolicy};
    use crate::search::rrf::ScoredResult;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_hormer_policy_update() {
        let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
        let policy = Arc::new(RwLock::new(NavigationPolicy::new(initial_weights, 0.1)));
        let hormer = Hormer::new(policy.clone());

        // Mock results with high relevance and diversity
        let results = vec![
            ScoredResult {
                id: "1".to_string(),
                content: "res 1".to_string(),
                score: 0.9,
                source: "working".to_string(),
                path: "path1".to_string(),
                updated_at: None,
            },
            ScoredResult {
                id: "2".to_string(),
                content: "res 2".to_string(),
                score: 0.8,
                source: "episodic".to_string(),
                path: "path2".to_string(),
                updated_at: None,
            },
        ];

        let initial_count = policy.read().await.update_count;
        hormer.update_from_interaction(initial_weights, &results).await;

        let updated_policy = policy.read().await;
        assert_eq!(updated_policy.update_count, initial_count + 1);

        // With high reward (relevance=0.85, diversity=1.0 -> reward=0.895)
        // Advantage = 0.895 - 0.5 = 0.395
        // Weights should increase (normalized)
        assert!(updated_policy.weights.is_valid());
    }

    #[tokio::test]
    async fn test_hormer_policy_no_update_on_low_advantage() {
        let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
        let policy = Arc::new(RwLock::new(NavigationPolicy::new(initial_weights, 0.1)));
        let hormer = Hormer::new(policy.clone());

        // Reward = Relevance * 0.7 + Diversity * 0.3
        // To get Reward = 0.5 (Advantage 0.0):
        // 2 results from same source (Diversity = 0.5)
        // One result with score 0.5, one with 0.4 (Relevance = 0.5)
        // Reward = 0.5 * 0.7 + 0.5 * 0.3 = 0.35 + 0.15 = 0.5
        let results = vec![
            ScoredResult {
                id: "1".to_string(),
                content: "res 1".to_string(),
                score: 0.5,
                source: "working".to_string(),
                path: "path1".to_string(),
                updated_at: None,
            },
            ScoredResult {
                id: "2".to_string(),
                content: "res 2".to_string(),
                score: 0.4,
                source: "working".to_string(),
                path: "path2".to_string(),
                updated_at: None,
            },
        ];

        let initial_count = policy.read().await.update_count;
        hormer.update_from_interaction(initial_weights, &results).await;

        let updated_policy = policy.read().await;
        assert_eq!(updated_policy.update_count, initial_count);
    }
}
