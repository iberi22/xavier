//! Cross-encoder reranker module for semantic context ranking.

pub mod cross_encoder;

pub use cross_encoder::{
    CrossEncoderConfig, CrossEncoderReranker, CrossEncoderResult, MockOnnxBackend,
    OnnxInferenceBackend,
};
