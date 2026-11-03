pub mod pooling;
pub mod tokenizer;

use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbedOrtError {
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),
    #[error("ONNX Runtime error: {0}")]
    Ort(String),
    #[error("Model not found at {0}")]
    ModelNotFound(String),
}

pub struct OrtEmbedder {
    session: Mutex<Session>,
    tokenizer: tokenizer::TextTokenizer,
}

impl OrtEmbedder {
    pub fn new(model_dir: &Path) -> Result<Self, EmbedOrtError> {
        let model_path = model_dir.join("model.onnx");
        if !model_path.exists() {
            return Err(EmbedOrtError::ModelNotFound(
                model_path.to_string_lossy().into_owned(),
            ));
        }

        let tokenizer = tokenizer::TextTokenizer::new(model_dir)
            .map_err(|e| EmbedOrtError::Tokenizer(e.to_string()))?;

        let session = Session::builder()
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?
            .with_intra_threads(4)
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?
            .commit_from_file(model_path)
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    pub fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedOrtError> {
        let (input_ids, attention_mask) = self
            .tokenizer
            .encode_query(text)
            .map_err(|e| EmbedOrtError::Tokenizer(e.to_string()))?;
        self.run_model(input_ids, attention_mask)
    }

    pub fn embed_document(
        &self,
        title: Option<&str>,
        content: &str,
    ) -> Result<Vec<f32>, EmbedOrtError> {
        let (input_ids, attention_mask) = self
            .tokenizer
            .encode_document(title, content)
            .map_err(|e| EmbedOrtError::Tokenizer(e.to_string()))?;
        self.run_model(input_ids, attention_mask)
    }

    fn run_model(
        &self,
        input_ids: Vec<i64>,
        attention_mask: Vec<i64>,
    ) -> Result<Vec<f32>, EmbedOrtError> {
        let seq_length = input_ids.len();

        let input_ids_tensor = Tensor::from_array(([1usize, seq_length], input_ids))
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?;
        let attention_mask_tensor =
            Tensor::from_array(([1usize, seq_length], attention_mask.clone()))
                .map_err(|e| EmbedOrtError::Ort(e.to_string()))?;

        let mut session = self
            .session
            .lock()
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?;

        let outputs = session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor
            ])
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?;

        let (shape, data) = outputs["last_hidden_state"]
            .try_extract_tensor::<f32>()
            .map_err(|e| EmbedOrtError::Ort(e.to_string()))?;
        let hidden_size = shape[2] as usize;

        let pooled = pooling::mean_pooling(data, &attention_mask, seq_length, hidden_size);
        let normalized = pooling::l2_normalize(pooled);

        Ok(normalized)
    }
}
