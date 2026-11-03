pub mod pooling;
pub mod tokenizer;

use ndarray::{Array2, CowArray};
use ort::{Environment, Session, SessionBuilder, Tensor, Value};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbedOrtError {
    #[error("Tokenizer error: {0}")]
    Tokenizer(String),
    #[error("ONNX Runtime error: {0}")]
    Ort(#[from] ort::Error),
    #[error("Model not found at {0}")]
    ModelNotFound(String),
}

pub struct OrtEmbedder {
    session: Session,
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

        // let env = Environment::builder().with_name("xavier-embed-ort").build()?.into_arc();
        let session = Session::builder()?
            .with_optimization_level(ort::GraphOptimizationLevel::Level3)?
            .with_intra_threads(4)?
            .commit_from_file(model_path)?;

        Ok(Self { session, tokenizer })
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

        let input_ids_array = Array2::from_shape_vec((1, seq_length), input_ids).unwrap();
        let attention_mask_array =
            Array2::from_shape_vec((1, seq_length), attention_mask.clone()).unwrap();

        let input_ids_tensor = Tensor::from_array(input_ids_array)?;
        let attention_mask_tensor = Tensor::from_array(attention_mask_array)?;

        let outputs = self.session.run(ort::inputs![
            "input_ids" => input_ids_tensor,
            "attention_mask" => attention_mask_tensor
        ]?)?;

        let last_hidden_state = outputs["last_hidden_state"].try_extract_tensor::<f32>()?;
        let hidden_size = last_hidden_state.shape()[2];

        let pooled = pooling::mean_pooling(
            last_hidden_state.as_slice().unwrap(),
            &attention_mask,
            seq_length,
            hidden_size,
        );
        let normalized = pooling::l2_normalize(pooled);

        Ok(normalized)
    }
}
