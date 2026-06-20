#[cfg(test)]
mod tests {
    use crate::agents::evolve::mutator::{Mutator, Mutation};
    use crate::agents::evolve::evaluator::Evaluator;
    use crate::agents::evolve::config::{BenchmarkType, EvolveConfig};
    use crate::agents::evolve::reflector::{Reflector, Insights};
    use crate::agents::evolve::EvolveModule;
    use crate::agents::evolve::experiment::HypothesisType;

    #[tokio::test]
    async fn test_mutator_changes_config_value() {
        let mutator = Mutator::new();
        let insights = Insights {
            best_metric: Some(0.8),
            best_description: Some("test".to_string()),
            improvement_rate: 0.1,
            suggestions: vec!["threshold".to_string()],
        };
        let mutations = mutator.generate_mutations(&insights).unwrap();
        assert!(!mutations.is_empty());
        let mutation = &mutations[0];
        match mutation {
            Mutation::Numeric { name, .. } => assert_eq!(name, "similarity_threshold"),
            _ => panic!("Expected numeric mutation"),
        }
    }

    #[tokio::test]
    async fn test_evaluator_accepts_improvement() {
        let evaluator = Evaluator::new(BenchmarkType::Custom);
        // post > pre (higher is better)
        assert!(evaluator.compare(0.8, 0.85, false));
        // post < pre (lower is better)
        assert!(evaluator.compare(0.8, 0.75, true));
    }

    #[tokio::test]
    async fn test_evaluator_rejects_regression() {
        let evaluator = Evaluator::new(BenchmarkType::Custom);
        // post < pre (higher is better)
        assert!(!evaluator.compare(0.8, 0.75, false));
        // post > pre (lower is better)
        assert!(!evaluator.compare(0.8, 0.85, true));
    }

    #[tokio::test]
    #[ignore = "EvolveModule::new() requires vec_store pool — needs mock infra"]
    async fn test_evolution_does_not_panic_on_empty_config() {
        let config = EvolveConfig::new("test".to_string());
        let evolve = EvolveModule::new(config).await.unwrap();
        // This test ensures that we can at least initialize and get state
        let state = evolve.state().await;
        assert_eq!(state.experiments_run, 0);
    }

    #[tokio::test]
    async fn test_mutator_toggle_flip() {
        let mutator = Mutator::new();
        let insights = Insights {
            suggestions: vec!["cache".to_string()],
            ..Default::default()
        };
        let mutations = mutator.generate_mutations(&insights).unwrap();
        assert!(mutations.iter().any(|m| matches!(m, Mutation::Toggle { .. })));
    }

    #[tokio::test]
    async fn test_mutator_structural_change() {
        let mutator = Mutator::new();
        let insights = Insights {
            suggestions: vec!["simplification".to_string()],
            ..Default::default()
        };
        let mutations = mutator.generate_mutations(&insights).unwrap();
        assert!(mutations.iter().any(|m| matches!(m, Mutation::Structural { .. })));
    }

    #[tokio::test]
    async fn test_evaluator_regression_check() {
        let evaluator = Evaluator::new(BenchmarkType::Custom);
        // Manual evaluate calls are difficult because they run scripts
        // But we can test is_regression logic if we had access to history (it's private Arc)
        // For now, testing compare is more reliable.
        assert!(!evaluator.compare(0.8, 0.801, false)); // 1% threshold
    }

    #[tokio::test]
    async fn test_mutation_to_hypothesis() {
        let mutator = Mutator::new();
        let mutation = Mutation::Numeric { name: "test".to_string(), old_value: 0.1, new_value: 0.2 };
        let hypothesis = mutator.mutation_to_hypothesis(&mutation);
        assert!(hypothesis.description.contains("test"));
        assert_eq!(hypothesis.hypothesis_type, HypothesisType::Hyperparameter);
    }

    #[tokio::test]
    #[ignore = "EvolveModule::new() requires vec_store pool — needs mock infra"]
    async fn test_full_evolution_cycle_logic_mock() {
        // Mocking the full cycle is hard due to file system and script dependencies
        // But we can verify the EvolveModule can be created and has the expected members
        let config = EvolveConfig::new("test".to_string());
        let evolve = EvolveModule::new(config).await.unwrap();
        let state = evolve.state().await;
        assert!(!state.running);
    }

    #[tokio::test]
    async fn test_reflector_analysis_empty() {
        let reflector = Reflector::new();
        let insights = reflector.analyze(&[]).await.unwrap();
        assert!(insights.best_metric.is_none());
    }
}
