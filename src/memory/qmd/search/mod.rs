//! QMD search: lexical, semantic, and hybrid retrieval
//!
//! Provides scoring, vector search, hybrid fusion, and embedding-based retrieval
//! for the QMD memory system.
//!
//! ## Module Structure
//!
//! | File | Responsibility |
//! |------|---------------|
//! | mod.rs | Module root, re-exports |
//! | scoring.rs | Lexical scoring + contextual boost |
//! | vector.rs | Vector (embedding) search |
//! | hybrid.rs | Hybrid (lexical + vector) fusion |
//! | resolution.rs | Document metadata resolution utilities |
//! | embedding.rs | Embedding computation |
//! | tests.rs | Integration tests |

pub mod embedding;
pub mod hybrid;
pub mod resolution;
pub mod scoring;
pub mod vector;

#[cfg(has_classifier_module)]
pub mod classifier;

#[cfg(test)]
pub mod tests;

// Re-export all public functions from sub-modules for backward compatibility.
pub use embedding::{query_with_embedding, query_with_embedding_filtered};
pub use hybrid::{
    bm25_search, classify_query_inline, get_query_weights, merge_ranked_candidates,
    multi_hop_context, query_filtered, query_with_adaptive_search, query_with_hybrid_search,
    search_hybrid_optimized, QueryClassWeights,
};
pub use resolution::{extract_answer, resolved_doc_metadata};
pub use scoring::{
    contextual_boost, lexical_score, locomo_lexical_score, memory_decay_penalty,
    memory_importance_score,
};
pub use vector::vsearch;
