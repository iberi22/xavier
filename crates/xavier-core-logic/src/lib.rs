//! Xavier Core Logic — Pure domain crate for BM25, RRF, retrieval scoring, and snippet processing.
//!
//! Free of I/O, database dependencies, or async runtimes. WASM-ready.

pub mod bm25;
pub mod rrf;
pub mod scoring;
pub mod snippet;
pub mod types;

pub use bm25::{score_documents, Bm25Params};
pub use rrf::{reciprocal_rank_fusion, reciprocal_rank_fusion_weighted};
pub use scoring::{
    calculate_recency_boost_factor, score_single_episodic, score_single_semantic,
    score_single_working, WorkingScoringParams,
};
pub use snippet::{clip_chars, extract, Excerpt, SnippetBudget};
pub use types::*;
