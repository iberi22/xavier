//! Expert router: picks the active mini-expert whose domain centroid is closest
//! (cosine similarity) to the query embedding, and invokes it through Ollama.
//!
//! Below the threshold no expert is chosen and the caller falls back to the
//! general model. Every invocation is logged to `mini_expert_invocations`.

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use serde::Serialize;

use crate::agents::mini_experts::{ExpertRecord, ExpertStore};
use crate::agents::provider_router::ProviderRouter;
use crate::embedding::Embedder;

pub const DEFAULT_THRESHOLD: f32 = 0.6;
pub const DEFAULT_KEEP_ALIVE: &str = "5m";

/// Ollama base URL (no `/v1` suffix) from `XAVIER_LOCAL_LLM_URL`.
pub fn ollama_base_url_from_env() -> String {
    let url = std::env::var("XAVIER_LOCAL_LLM_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
    let trimmed = url.trim().trim_end_matches('/');
    let base = trimmed.strip_suffix("/v1").unwrap_or(trimmed);
    if base.is_empty() {
        "http://localhost:11434".to_string()
    } else {
        base.to_string()
    }
}

/// Router settings.
#[derive(Debug, Clone)]
pub struct ExpertRouterConfig {
    /// Minimum cosine similarity to pick an expert.
    pub threshold: f32,
    /// Ollama `keep_alive` for expert models (cold experts unload after this).
    pub keep_alive: String,
    pub ollama_base_url: String,
    /// Identifier of the embedding model; part of the centroid cache key.
    pub embedding_model: String,
}

impl Default for ExpertRouterConfig {
    fn default() -> Self {
        Self {
            threshold: DEFAULT_THRESHOLD,
            keep_alive: DEFAULT_KEEP_ALIVE.to_string(),
            ollama_base_url: "http://localhost:11434".to_string(),
            embedding_model: String::new(),
        }
    }
}

/// Parses a threshold: finite values are clamped to [-1, 1] (cosine range); NaN,
/// infinity and unparsable input fall back to the default.
pub fn parse_threshold(raw: Option<&str>) -> f32 {
    raw.and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(-1.0, 1.0))
        .unwrap_or(DEFAULT_THRESHOLD)
}

/// Best-effort identifier of the configured embedding model/provider.
fn embedding_model_from_env() -> String {
    [
        "XAVIER_EMBEDDER",
        "XAVIER_EMBEDDING_PROVIDER_MODE",
        "XAVIER_EMBEDDING_LOCAL_MODEL",
        "XAVIER_EMBEDDING_CLOUD_MODEL",
        "XAVIER_EMBEDDING_MODEL",
    ]
    .iter()
    .map(|k| std::env::var(k).unwrap_or_default())
    .collect::<Vec<_>>()
    .join("|")
}

impl ExpertRouterConfig {
    /// `XAVIER_EXPERT_THRESHOLD`, `XAVIER_EXPERT_KEEP_ALIVE`, `XAVIER_LOCAL_LLM_URL`.
    pub fn from_env() -> Self {
        let threshold = parse_threshold(std::env::var("XAVIER_EXPERT_THRESHOLD").ok().as_deref());
        let keep_alive = std::env::var("XAVIER_EXPERT_KEEP_ALIVE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_KEEP_ALIVE.to_string());
        Self {
            threshold,
            keep_alive,
            ollama_base_url: ollama_base_url_from_env(),
            embedding_model: embedding_model_from_env(),
        }
    }
}

/// A selected expert and its similarity score.
#[derive(Debug, Clone)]
pub struct ExpertMatch {
    pub record: ExpertRecord,
    /// Cosine similarity; `None` when chosen by an explicit domain hint.
    pub score: Option<f32>,
    /// `"domain"` (explicit hint) or `"similarity"` (centroid match).
    pub matched_by: &'static str,
}

/// Result of `ask`: either an expert answered or none matched.
#[derive(Debug, Clone, Serialize)]
pub struct AskOutcome {
    pub answer: Option<String>,
    pub expert: Option<String>,
    pub version: Option<String>,
    pub score: Option<f32>,
    pub matched_by: Option<String>,
}

impl AskOutcome {
    pub fn no_expert() -> Self {
        Self {
            answer: None,
            expert: None,
            version: None,
            score: None,
            matched_by: None,
        }
    }
}

/// Cosine similarity; 0.0 for mismatched/empty/zero vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Lazily-built process-wide embedder from the daemon environment.
pub async fn shared_embedder() -> Option<Arc<dyn Embedder>> {
    static CELL: tokio::sync::OnceCell<Arc<dyn Embedder>> = tokio::sync::OnceCell::const_new();
    CELL.get_or_try_init(crate::embedding::build_embedder_from_env)
        .await
        .map(Arc::clone)
        .map_err(|e| tracing::warn!("expert router: embedder unavailable: {e}"))
        .ok()
}

pub struct ExpertRouter {
    provider: ProviderRouter,
    store: ExpertStore,
    embedder: Option<Arc<dyn Embedder>>,
    config: ExpertRouterConfig,
}

impl ExpertRouter {
    pub fn new(
        store: ExpertStore,
        embedder: Option<Arc<dyn Embedder>>,
        config: ExpertRouterConfig,
    ) -> Self {
        Self {
            provider: ProviderRouter::new(vec![]),
            store,
            embedder,
            config,
        }
    }

    /// Production wiring: default store, daemon embedder, env config.
    pub async fn from_env() -> Result<Self> {
        Ok(Self::new(
            ExpertStore::open_default()?,
            shared_embedder().await,
            ExpertRouterConfig::from_env(),
        ))
    }

    pub fn store(&self) -> &ExpertStore {
        &self.store
    }

    /// Cached centroid of `domain` for the current embedder (`dimension` components).
    async fn centroid(
        &self,
        embedder: &dyn Embedder,
        domain: &str,
        dimension: usize,
    ) -> Result<Vec<f32>> {
        let model = &self.config.embedding_model;
        if let Some(c) = self.store.get_centroid(domain, domain, model, dimension)? {
            return Ok(c);
        }
        let emb = embedder.encode(domain).await?;
        self.store.set_centroid(domain, domain, model, &emb)?;
        Ok(emb)
    }

    /// Picks an expert. With `domain` the active expert of that domain (case-insensitive)
    /// is chosen directly; without it the best centroid at or above the threshold wins.
    /// A centroid that cannot be computed is logged and skipped, not fatal.
    pub async fn select(&self, query: &str, domain: Option<&str>) -> Result<Option<ExpertMatch>> {
        let active = self.store.list_active()?;
        if let Some(d) = domain.map(str::trim).filter(|d| !d.is_empty()) {
            // Deterministic among same-domain experts: newest active version, then name.
            return Ok(active
                .into_iter()
                .filter(|r| r.domain.eq_ignore_ascii_case(d) || r.name.eq_ignore_ascii_case(d))
                .min_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.name.cmp(&b.name))
                })
                .map(|record| ExpertMatch {
                    record,
                    score: None,
                    matched_by: "domain",
                }));
        }
        let Some(embedder) = self.embedder.as_deref() else {
            return Ok(None);
        };
        if active.is_empty() {
            return Ok(None);
        }
        let q = embedder.encode(query).await?;
        let mut best: Option<ExpertMatch> = None;
        for record in active {
            if record.domain.trim().is_empty() {
                continue;
            }
            let c = match self.centroid(embedder, &record.domain, q.len()).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        "expert router: skipping '{}': centroid error: {e}",
                        record.name
                    );
                    continue;
                }
            };
            if c.len() != q.len() {
                tracing::warn!(
                    "expert router: skipping '{}': centroid has {} dims, query has {}",
                    record.name,
                    c.len(),
                    q.len()
                );
                continue;
            }
            let score = cosine_similarity(&q, &c);
            if score >= self.config.threshold
                && best
                    .as_ref()
                    .is_none_or(|b| score > b.score.unwrap_or(f32::MIN))
            {
                best = Some(ExpertMatch {
                    record,
                    score: Some(score),
                    matched_by: "similarity",
                });
            }
        }
        Ok(best)
    }

    /// Invokes a specific expert version via Ollama chat and logs the call.
    pub async fn invoke_record(&self, record: &ExpertRecord, prompt: &str) -> Result<String> {
        let start = Instant::now();
        let result = self
            .provider
            .invoke_ollama_chat(
                &self.config.ollama_base_url,
                &record.ollama_model,
                prompt,
                &self.config.keep_alive,
            )
            .await;
        let latency = start.elapsed().as_millis() as u64;
        if let Err(e) = self.store.log_invocation(
            &record.name,
            &record.version,
            prompt,
            latency,
            result.is_ok(),
        ) {
            tracing::warn!("failed to log mini-expert invocation: {e}");
        }
        Ok(result?)
    }

    /// Routes `prompt` to an expert and answers; `AskOutcome::no_expert()` when none matches.
    pub async fn ask(&self, prompt: &str, domain: Option<&str>) -> Result<AskOutcome> {
        let Some(m) = self.select(prompt, domain).await? else {
            return Ok(AskOutcome::no_expert());
        };
        let answer = self.invoke_record(&m.record, prompt).await?;
        Ok(AskOutcome {
            answer: Some(answer),
            expert: Some(m.record.name),
            version: Some(m.record.version),
            score: m.score,
            matched_by: Some(m.matched_by.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::mini_experts::NewExpert;
    use crate::embedding::EmbeddingError;
    use async_trait::async_trait;

    /// Deterministic 3-D embedder: "rust" -> x axis, "cooking" -> y axis.
    struct AxisEmbedder;

    #[async_trait]
    impl Embedder for AxisEmbedder {
        async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            let t = text.to_lowercase();
            let x = if t.contains("rust") { 1.0 } else { 0.0 };
            let y = if t.contains("cook") { 1.0 } else { 0.0 };
            // Constant third axis so unrelated text is not a zero vector.
            Ok(vec![x, y, 0.3])
        }
        fn dimension(&self) -> usize {
            3
        }
    }

    fn setup(threshold: f32, base_url: &str) -> (tempfile::TempDir, ExpertRouter) {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        for (name, domain) in [
            ("rust-exp", "rust programming"),
            ("cook-exp", "cooking recipes"),
        ] {
            store
                .add_version(
                    NewExpert {
                        name: name.into(),
                        domain: domain.into(),
                        ..Default::default()
                    },
                    true,
                )
                .unwrap();
        }
        let cfg = ExpertRouterConfig {
            threshold,
            keep_alive: "5m".into(),
            ollama_base_url: base_url.into(),
            embedding_model: "axis".into(),
        };
        (
            dir,
            ExpertRouter::new(store, Some(Arc::new(AxisEmbedder)), cfg),
        )
    }

    #[test]
    fn test_cosine_similarity() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 2.0]), 0.0);
    }

    #[tokio::test]
    async fn test_select_by_similarity_and_threshold() {
        let (_d, router) = setup(0.6, "http://unused");
        let m = router
            .select("how to write rust traits", None)
            .await
            .unwrap();
        assert_eq!(m.unwrap().record.name, "rust-exp");
        let m = router.select("best cooking tips", None).await.unwrap();
        assert_eq!(m.unwrap().record.name, "cook-exp");
        // Unrelated query: both centroids score ~0.29 -> below threshold.
        assert!(router
            .select("weather today", None)
            .await
            .unwrap()
            .is_none());

        // Same query, impossible threshold -> none; zero threshold -> some expert.
        let (_d2, strict) = setup(1.01, "http://unused");
        assert!(strict.select("rust", None).await.unwrap().is_none());
        let (_d3, lax) = setup(0.0, "http://unused");
        assert!(lax.select("weather today", None).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_domain_hint_and_no_embedder() {
        let (_d, router) = setup(0.99, "http://unused");
        let m = router
            .select("anything", Some("Cooking Recipes"))
            .await
            .unwrap();
        assert_eq!(m.unwrap().record.name, "cook-exp");
        assert!(router
            .select("anything", Some("physics"))
            .await
            .unwrap()
            .is_none());

        let no_emb = ExpertRouter::new(router.store().clone(), None, ExpertRouterConfig::default());
        assert!(no_emb.select("rust", None).await.unwrap().is_none());
    }

    #[test]
    fn test_parse_threshold_rejects_non_finite_and_clamps() {
        assert_eq!(parse_threshold(None), DEFAULT_THRESHOLD);
        assert_eq!(parse_threshold(Some("NaN")), DEFAULT_THRESHOLD);
        assert_eq!(parse_threshold(Some("inf")), DEFAULT_THRESHOLD);
        assert_eq!(parse_threshold(Some("-inf")), DEFAULT_THRESHOLD);
        assert_eq!(parse_threshold(Some("junk")), DEFAULT_THRESHOLD);
        assert_eq!(parse_threshold(Some(" 0.75 ")), 0.75);
        assert_eq!(parse_threshold(Some("5")), 1.0);
        assert_eq!(parse_threshold(Some("-3")), -1.0);
    }

    /// Like `AxisEmbedder` but fails for any text containing "broken".
    struct FlakyEmbedder;

    #[async_trait]
    impl Embedder for FlakyEmbedder {
        async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            if text.contains("broken") {
                return Err(EmbeddingError::Network("boom".into()));
            }
            AxisEmbedder.encode(text).await
        }
        fn dimension(&self) -> usize {
            3
        }
    }

    /// 4-D variant: a different embedder (new dimension) over the same store.
    struct Axis4Embedder;

    #[async_trait]
    impl Embedder for Axis4Embedder {
        async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
            let mut v = AxisEmbedder.encode(text).await?;
            v.push(0.0);
            Ok(v)
        }
        fn dimension(&self) -> usize {
            4
        }
    }

    #[tokio::test]
    async fn test_one_bad_centroid_does_not_abort_routing() {
        let (_d, router) = setup(0.6, "http://unused");
        router
            .store()
            .add_version(
                NewExpert {
                    name: "bad-exp".into(),
                    domain: "broken domain".into(),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        let flaky = ExpertRouter::new(
            router.store().clone(),
            Some(Arc::new(FlakyEmbedder)),
            ExpertRouterConfig {
                embedding_model: "axis".into(),
                ..Default::default()
            },
        );
        let m = flaky.select("how to write rust", None).await.unwrap();
        assert_eq!(m.unwrap().record.name, "rust-exp");
    }

    #[tokio::test]
    async fn test_embedder_change_recomputes_centroids() {
        let (_d, router) = setup(0.6, "http://unused");
        // Warm the cache with the 3-D embedder.
        assert!(router.select("rust", None).await.unwrap().is_some());
        // New embedder (4-D) over the same store: stale 3-D centroids must be recomputed.
        let router4 = ExpertRouter::new(
            router.store().clone(),
            Some(Arc::new(Axis4Embedder)),
            ExpertRouterConfig {
                embedding_model: "axis4".into(),
                ..Default::default()
            },
        );
        let m = router4.select("how to write rust", None).await.unwrap();
        assert_eq!(m.unwrap().record.name, "rust-exp");
        // Even with an unchanged model id, a dimension change is a miss, not a silent 0.
        let router4b = ExpertRouter::new(
            router.store().clone(),
            Some(Arc::new(Axis4Embedder)),
            ExpertRouterConfig {
                embedding_model: "axis".into(),
                ..Default::default()
            },
        );
        let m = router4b.select("how to write rust", None).await.unwrap();
        assert_eq!(m.unwrap().record.name, "rust-exp");
    }

    #[tokio::test]
    async fn test_match_provenance_and_deterministic_domain_choice() {
        let (_d, router) = setup(0.6, "http://unused");
        let sim = router.select("rust traits", None).await.unwrap().unwrap();
        assert_eq!(sim.matched_by, "similarity");
        assert!(sim.score.is_some_and(|s| s > 0.6 && s <= 1.0));

        let hint = router
            .select("anything", Some("rust programming"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hint.matched_by, "domain");
        assert_eq!(hint.score, None);

        // Two active experts share a domain: the newest wins, every time.
        std::thread::sleep(std::time::Duration::from_millis(5));
        router
            .store()
            .add_version(
                NewExpert {
                    name: "aaa-newer".into(),
                    domain: "rust programming".into(),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        for _ in 0..3 {
            let m = router
                .select("anything", Some("rust programming"))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(m.record.name, "aaa-newer");
        }
    }

    #[tokio::test]
    async fn test_ask_invokes_expert_logs_and_falls_back() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJson(
                serde_json::json!({"model": "rust-exp:v1", "keep_alive": "5m"}),
            ))
            .with_status(200)
            .with_body(r#"{"message":{"content":"use traits"}}"#)
            .create_async()
            .await;
        let (_d, router) = setup(0.6, &server.url());

        let out = router.ask("explain rust traits", None).await.unwrap();
        assert_eq!(out.answer.as_deref(), Some("use traits"));
        assert_eq!(out.expert.as_deref(), Some("rust-exp"));
        assert_eq!(out.version.as_deref(), Some("v1"));
        assert_eq!(out.matched_by.as_deref(), Some("similarity"));
        mock.assert_async().await;

        let log = router.store().invocations(10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].ok);
        assert_eq!(
            log[0].query_sha256,
            crate::agents::mini_experts::sha256_hex("explain rust traits")
        );

        let none = router.ask("weather today", None).await.unwrap();
        assert!(none.answer.is_none() && none.expert.is_none());
        assert_eq!(router.store().invocations(10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_failed_invocation_is_logged_not_ok() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/api/chat")
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;
        let (_d, router) = setup(0.6, &server.url());
        assert!(router.ask("rust", None).await.is_err());
        let log = router.store().invocations(10).unwrap();
        assert_eq!(log.len(), 1);
        assert!(!log[0].ok);
    }
}
