//! Search API endpoints
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::{extract::Extension, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use crate::{
    memory::{
        query_engine::{MemoryQueryEngine, SearchQuery, SearchResultItem},
        schema::MemoryQueryFilters,
    },
    workspace::WorkspaceContext,
};

#[derive(Debug, Deserialize)]
pub struct HybridSearchRequest {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub rrf_k: Option<u32>,
    #[serde(default = "default_keyword_weight")]
    pub keyword_weight: f32,
    #[serde(default = "default_vector_weight")]
    pub vector_weight: f32,
    #[serde(default)]
    pub filters: Option<MemoryQueryFilters>,
    #[serde(default)]
    pub include_embedding: Option<bool>,
}

fn default_keyword_weight() -> f32 {
    0.5
}

fn default_vector_weight() -> f32 {
    0.5
}

fn default_limit() -> usize {
    10
}

pub type SearchResult = SearchResultItem;

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub query_vector: Option<Vec<f32>>,
    pub total_available: usize,
    pub search_type: String,
}

/// Hybrid search.
pub async fn hybrid_search(
    Extension(workspace): Extension<WorkspaceContext>,
    Json(request): Json<HybridSearchRequest>,
) -> impl IntoResponse {
    let engine = MemoryQueryEngine::new();
    let query = SearchQuery {
        query: request.query,
        limit: request.limit,
        rrf_k: request.rrf_k,
        keyword_weight: request.keyword_weight,
        vector_weight: request.vector_weight,
        filters: request.filters,
        include_embedding: request.include_embedding,
        ..Default::default()
    };

    let search_results = engine
        .search(&workspace.workspace.memory, query)
        .await
        .unwrap_or_else(|_| crate::memory::query_engine::SearchResults {
            results: Vec::new(),
            query_vector: None,
            total_available: 0,
            search_type: "hybrid".to_string(),
        });

    let response = SearchResponse {
        results: search_results.results,
        query_vector: search_results.query_vector,
        total_available: search_results.total_available,
        search_type: search_results.search_type,
    };

    Json(response)
}
