use crate::embedding::{Embedder, EmbeddingError};

pub const DEFAULT_GLLM_MODEL: &str = "all-MiniLM-L6-v2";

pub fn normalize_model_name(raw: &str) -> String {
    raw.to_string()
}

pub fn dimension_for_model(model: &str) -> usize {
    if model.contains("minilm") {
        384
    } else {
        768
    }
}

pub struct GllmEmbedder {
    model: String,
    dimension: usize,
}

impl GllmEmbedder {
    pub fn new(model: String, dimension: usize) -> Result<Self, EmbeddingError> {
        Ok(Self { model, dimension })
    }
}

#[async_trait::async_trait]
impl Embedder for GllmEmbedder {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(Vec::new())
    }
    fn dimension(&self) -> usize {
        self.dimension
    }
}
