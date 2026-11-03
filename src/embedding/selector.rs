use crate::embedding::{build_embedder_from_env, Embedder, EmbeddingError};
use crate::server::alerts::SYSTEM_ALERTS;
use reqwest;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentModality {
    Text,
    Code,
    Documentation,
    Audit,
}

pub struct EmbeddingSelector {
    text_embedder: Arc<dyn Embedder>,
    code_embedder: Arc<dyn Embedder>,
    http_client: reqwest::Client,
}

impl EmbeddingSelector {
    pub async fn new() -> Result<Self, EmbeddingError> {
        let text_embedder = match build_embedder_from_env().await {
            Ok(embedder) if embedder.dimension() > 0 => embedder,
            _ => {
                let ollama_embedder = crate::embedding::ollama::OllamaEmbedder::new(
                    "embeddinggemma".to_string(),
                    "http://localhost:11434/api/embed".to_string(),
                    768,
                    Duration::from_secs(30),
                )
                .map_err(|e| EmbeddingError::Config(e.to_string()))?;
                Arc::new(ollama_embedder)
            }
        };

        let ollama_embedder = crate::embedding::ollama::OllamaEmbedder::new(
            "qwen3-embedding:0.6b".to_string(),
            "http://localhost:11435/api/embed".to_string(),
            1024,
            Duration::from_secs(30),
        )
        .map_err(|e| EmbeddingError::Config(e.to_string()))?;

        let code_embedder: Arc<dyn Embedder> = Arc::new(ollama_embedder);
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap_or_default();

        Ok(Self {
            text_embedder,
            code_embedder,
            http_client,
        })
    }

    pub fn select_modality(content: &str, extension: Option<&str>) -> ContentModality {
        if let Some(ext) = extension {
            match ext {
                "rs" | "py" | "js" | "ts" | "c" | "cpp" | "java" | "go" => {
                    return ContentModality::Code
                }
                "md" | "txt" => return ContentModality::Text,
                _ => {}
            }
        }

        if content.contains("fn ") || content.contains("def ") || content.contains("function ") {
            return ContentModality::Code;
        }

        ContentModality::Text
    }

    pub async fn get_embedder(&self, modality: ContentModality) -> Arc<dyn Embedder> {
        match modality {
            ContentModality::Code => {
                let is_qwen_online = match self
                    .http_client
                    .get(
                        std::env::var("_XAVIER_TEST_OLLAMA_PROBE_URL")
                            .unwrap_or_else(|_| "http://localhost:11435/v1/models".to_string()),
                    )
                    .send()
                    .await
                {
                    Ok(resp) => resp.status().is_success(),
                    Err(_) => false,
                };

                if is_qwen_online {
                    self.code_embedder.clone()
                } else {
                    SYSTEM_ALERTS.push_alert(
                        "WARN",
                        "Qwen 1024d is offline, falling back to Text 768d",
                        "embedding",
                    );
                    self.text_embedder.clone()
                }
            }
            _ => self.text_embedder.clone(),
        }
    }
}
