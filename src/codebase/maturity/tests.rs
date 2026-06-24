#[cfg(test)]
mod tests {
    use crate::codebase::maturity::config::{AnchorManifest, FeatureAnchor, SubcomponentAnchor, StaticCheck};
    use crate::codebase::maturity::engine::MaturityEngine;
    use tempfile::tempdir;
    use std::fs;

    #[tokio::test]
    async fn test_maturity_engine_scan() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_str().unwrap();

        // Create a dummy file with a symbol
        let file_path = dir.path().join("lib.rs");
        fs::write(&file_path, "pub struct MyFeature;").unwrap();

        let manifest = AnchorManifest {
            features: vec![FeatureAnchor {
                id: "test-feature".to_string(),
                name: "Test Feature".to_string(),
                subcomponents: vec![SubcomponentAnchor {
                    name: "Sub1".to_string(),
                    weight: 100,
                    static_checks: vec![StaticCheck {
                        symbol: "MyFeature".to_string(),
                        required: true,
                    }],
                    required_feature: None,
                    test_anchors: vec![],
                }],
            }],
        };

        let engine = MaturityEngine::new(root, manifest);
        let results = engine.scan().await.unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "test-feature");
        assert_eq!(results[0].overall, 100.0);
    }
}
