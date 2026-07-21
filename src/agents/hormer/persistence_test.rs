// SPDX-License-Identifier: MIT OR LICENSE-MESH
#[cfg(test)]
mod persistence_tests {
    use super::super::*;
    use crate::retrieval::{LayerWeights, NavigationPolicy};
    use crate::search::rrf::ScoredResult;
    use crate::settings::XavierSettings;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_hormer_persistence() {
        let config_path = "config/test_hormer_config.json";
        std::env::set_var("XAVIER_CONFIG_PATH", config_path);

        // Ensure clean start
        let _ = std::fs::remove_file(config_path);

        let initial_weights = LayerWeights::new(0.3, 0.3, 0.4);
        let policy = Arc::new(RwLock::new(NavigationPolicy::new(
            initial_weights,
            crate::retrieval::policy::TraversalWeights::default(),
            0.1,
        )));
        let hormer = Hormer::new(Arc::clone(&policy));

        let results = vec![
            ScoredResult {
                id: "1".to_string(),
                content: "Very relevant".to_string(),
                score: 1.0,
                source: "working".to_string(),
                path: "p1".to_string(),
                updated_at: None,
                zone: None,
            },
            ScoredResult {
                id: "2".to_string(),
                content: "Very relevant too".to_string(),
                score: 1.0,
                source: "episodic".to_string(),
                path: "p2".to_string(),
                updated_at: None,
                zone: None,
            },
        ];

        hormer
            .update_from_interaction(initial_weights, &results, None)
            .await;

        // Check if file exists and contains updated weights
        let settings = XavierSettings::load()
            .unwrap()
            .expect("Settings should be loaded");
        println!(
            "Working weight: {}",
            settings.retrieval.learned_policy.working_weight
        );
        println!(
            "Update count: {}",
            settings.retrieval.learned_policy.update_count
        );

        assert!(
            settings.retrieval.learned_policy.working_weight != 0.3
                || settings.retrieval.learned_policy.update_count > 0
        );

        std::fs::remove_file(config_path).unwrap();
    }
}
