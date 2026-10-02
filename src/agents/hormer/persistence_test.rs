#[cfg(test)]
mod persistence_tests {
    use super::super::*;
    use crate::retrieval::{LayerWeights, NavigationPolicy};
    use crate::search::rrf::ScoredResult;
    use crate::settings::XavierSettings;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    #[serial_test::serial]
    async fn test_hormer_persistence() {
        let _temp_env = crate::settings::tests::TempEnv::new();
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config_path = temp_dir.path().join("test_hormer_config.json");
        // #2801: HORMER persists to the runtime state file, never to the
        // (versioned, read-only) config file.
        let state_path = temp_dir.path().join("xavier.runtime.json");
        let default_settings = XavierSettings::default();
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&default_settings).unwrap(),
        )
        .unwrap();
        std::env::set_var("XAVIER_CONFIG_PATH", &config_path);
        std::env::set_var("XAVIER_RUNTIME_STATE_PATH", &state_path);

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

        // Check the state file exists and contains the updated weights
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

        // The config file kept the defaults it was written with.
        let config_after: crate::settings::XavierSettings =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(
            config_after.retrieval.learned_policy.update_count, 0,
            "HORMER must not write the learned policy into the config file"
        );

        std::env::remove_var("XAVIER_RUNTIME_STATE_PATH");
    }
}
