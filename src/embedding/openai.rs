//! OpenAI embedding API integration
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use std::fmt;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::embedding::{Embedder, EmbeddingError};

pub struct OpenAICompatibleEmbedder {
    client: Client,
    api_key: Option<String>,
    model: String,
    endpoint: String,
    dimension: usize,
}

impl fmt::Debug for OpenAICompatibleEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAICompatibleEmbedder")
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("endpoint", &self.endpoint)
            .field("dimension", &self.dimension)
            .finish()
    }
}

impl OpenAICompatibleEmbedder {
    /// New.
    pub fn new(
        api_key: Option<String>,
        model: String,
        endpoint: String,
        dimension: usize,
        timeout: Duration,
    ) -> Result<Self, EmbeddingError> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| EmbeddingError::Network(error.to_string()))?;

        Ok(Self {
            client,
            api_key: api_key.filter(|value| !value.trim().is_empty()),
            model,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            dimension,
        })
    }
}

#[derive(Debug, Serialize)]
struct EmbeddingRequest<'a> {
    input: &'a str,
    model: &'a str,
}

#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    #[serde(default)]
    data: Vec<EmbeddingData>,
    embedding: Option<Vec<f32>>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingResponse {
    fn first_embedding(self) -> Option<Vec<f32>> {
        self.embedding
            .or_else(|| self.data.into_iter().next().map(|item| item.embedding))
    }
}

#[async_trait::async_trait]
impl Embedder for OpenAICompatibleEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json");

        if let Some(api_key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }

        let primary_res = request
            .json(&EmbeddingRequest {
                input: text,
                model: &self.model,
            })
            .send()
            .await;

        let primary_res = match primary_res {
            Ok(resp) => resp.error_for_status(),
            Err(e) => Err(e),
        };

        match primary_res {
            Ok(response) => {
                let body: EmbeddingResponse = response
                    .json()
                    .await
                    .map_err(|error| EmbeddingError::Parse(error.to_string()))?;
                Ok(body.first_embedding().unwrap_or_default())
            }
            Err(primary_err) => {
                crate::embedding::increment_embedding_error_count();

                // Fallback attempt: if XAVIER_EMBEDDING_LOCAL_URL is configured and different from endpoint, try once
                if let Ok(local_url) = std::env::var("XAVIER_EMBEDDING_LOCAL_URL") {
                    let normalized_local = local_url.trim_end_matches('/').to_string();
                    if !normalized_local.is_empty() && normalized_local != self.endpoint {
                        tracing::warn!(
                            "Primary cloud embedding endpoint failed ({}); attempting single fallback to local URL: {}",
                            primary_err,
                            normalized_local
                        );
                        let local_endpoint = if normalized_local.ends_with("/v1/embeddings")
                            || normalized_local.ends_with("/api/embed")
                        {
                            normalized_local
                        } else {
                            format!("{}/v1/embeddings", normalized_local)
                        };

                        let fallback_req = self
                            .client
                            .post(&local_endpoint)
                            .header("Content-Type", "application/json")
                            .json(&EmbeddingRequest {
                                input: text,
                                model: &self.model,
                            });

                        if let Ok(fallback_resp) = fallback_req.send().await {
                            if let Ok(valid_resp) = fallback_resp.error_for_status() {
                                if let Ok(body) = valid_resp.json::<EmbeddingResponse>().await {
                                    tracing::info!(
                                        "Single local fallback succeeded after cloud failure"
                                    );
                                    return Ok(body.first_embedding().unwrap_or_default());
                                }
                            }
                        }
                    }
                }

                Err(EmbeddingError::Network(primary_err.to_string()))
            }
        }
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}
