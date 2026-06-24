use crate::adapters::outbound::code_graph_fallback::FallbackCodeGraphAdapter;
use crate::ports::outbound::code_graph::CodeGraphPort;
use crate::codebase::maturity::config::AnchorManifest;
use crate::codebase::maturity::scorer::{MaturityScorer, ScoredFeature};
use crate::codebase::maturity::reporter::MaturityReporter;
use crate::AppState;
use anyhow::Result;
use std::sync::Arc;

pub struct MaturityEngine {
    adapter: Arc<dyn CodeGraphPort>,
    manifest: AnchorManifest,
    _codebase_root: String,
    scorer: MaturityScorer,
    reporter: MaturityReporter,
    app_state: Option<Arc<AppState>>,
}

impl MaturityEngine {
    pub fn new(root: &str, manifest: AnchorManifest) -> Self {
        let adapter = Arc::new(FallbackCodeGraphAdapter::new(root));
        let scorer = MaturityScorer::new(adapter.clone());
        let reporter = MaturityReporter::new(root);
        Self {
            adapter,
            manifest,
            _codebase_root: root.to_string(),
            scorer,
            reporter,
            app_state: None,
        }
    }

    pub fn with_app_state(mut self, state: Arc<AppState>) -> Self {
        self.app_state = Some(state);
        self
    }

    pub fn with_adapter(mut self, adapter: Arc<dyn CodeGraphPort>) -> Self {
        self.adapter = adapter.clone();
        self.scorer = MaturityScorer::new(adapter);
        self
    }

    pub async fn scan(&self) -> Result<Vec<ScoredFeature>> {
        let mut results = Vec::new();

        for feature in &self.manifest.features {
            let (memory_evidence, issue_evidence) = self.get_evidence(&feature.id).await?;

            let scored = self.scorer.score_feature(feature, memory_evidence, issue_evidence).await?;
            results.push(scored);
        }
        Ok(results)
    }

    async fn get_evidence(&self, feature_id: &str) -> Result<(f64, f64)> {
        if let Some(ref state) = self.app_state {
            // Real Xavier RAG Memory search for feature evidence
            // We search for the feature_id in the memory store
            let query = format!("evidence of feature {}", feature_id);

            // This is a simplified call to the search engine
            // In a real scenario, we would use state.code_query or a memory search port
            let results = state.code_query.search(&query, 5).map(|_| vec![1]).unwrap_or_else(|_| vec![]);

            let memory_ratio = if results.is_empty() { 0.1 } else { 0.8 };
            let issue_ratio = 0.5; // Placeholder for conversation evidence

            Ok((memory_ratio, issue_ratio))
        } else {
            Ok((0.0, 0.0))
        }
    }

    pub async fn verify_design(&self, feature_id: &str) -> Result<bool> {
        self.adapter.verify_design(feature_id).await
    }

    pub async fn design_gaps(&self, features_path: &str) -> Result<Vec<String>> {
        self.adapter.get_gaps(features_path).await
    }

    pub fn report(&self, results: &[ScoredFeature]) -> Result<()> {
        self.reporter.generate_report(results)
    }
}
