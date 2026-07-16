use crate::embedding::{Embedder, EmbeddingError};

pub struct OpenAICompatibleEmbedder {
    model: String,
    dimension: usize,
}

impl OpenAICompatibleEmbedder {
    pub fn new(
        _api_key: Option<String>,
        model: String,
        _endpoint: String,
        dimension: usize,
    ) -> Result<Self, EmbeddingError> {
        Ok(Self { model, dimension })
    }
}

#[async_trait::async_trait]
impl Embedder for OpenAICompatibleEmbedder {
    async fn encode(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
        Ok(Vec::new())
    }
    fn dimension(&self) -> usize {
        self.dimension
    }
}
