//! Maloca Belief Graph Confidence API (MS-005)
//!
//! Exposes the belief graph (nodes + edges) with confidence scores and
//! highest-confidence path queries. Consumed by swal-backoffice GraphPage /
//! CouncilPage (`GET /maloca/beliefs`).
//!
//! Contract:
//!   GET /maloca/beliefs                    -> { nodes, edges, stats }
//!   GET /maloca/beliefs/path?from=A&to=B   -> { path: [edges], total_confidence }

use axum::extract::Query;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::workspace::WorkspaceContext;

#[derive(Debug, Deserialize)]
pub struct BeliefPathQuery {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct BeliefNodeDto {
    pub id: String,
    pub concept: String,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_family: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct BeliefEdgeDto {
    pub id: String,
    pub source: String,
    pub target: String,
    pub relation_type: String,
    pub weight: f32,
    pub confidence_score: f32,
    pub provenance_id: String,
    pub is_inferred: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contradicts_edge_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BeliefStats {
    pub nodes: usize,
    pub edges: usize,
    pub avg_confidence: f32,
    pub high_confidence_edges: usize,
    pub contradictions: usize,
}

/// Full belief graph snapshot + aggregate stats.
pub async fn beliefs_snapshot(Extension(ctx): Extension<WorkspaceContext>) -> impl IntoResponse {
    let graph = &ctx.workspace.belief_graph;
    let graph = graph.read().await;
    let nodes = graph.list_nodes();
    let edges = graph.get_edges();

    let node_dtos: Vec<BeliefNodeDto> = nodes
        .iter()
        .map(|n| BeliefNodeDto {
            id: n.id.clone(),
            concept: n.concept.clone(),
            confidence: n.confidence,
            language_family: n.language_family.clone(),
            created_at: n.created_at.to_rfc3339(),
        })
        .collect();

    let edge_dtos: Vec<BeliefEdgeDto> = edges
        .iter()
        .map(|e| BeliefEdgeDto {
            id: e.id.clone(),
            source: e.source.clone(),
            target: e.target.clone(),
            relation_type: e.relation_type.clone(),
            weight: e.weight,
            confidence_score: e.confidence_score,
            provenance_id: e.provenance_id.clone(),
            is_inferred: e.is_inferred,
            contradicts_edge_id: e.contradicts_edge_id.clone(),
        })
        .collect();

    let avg_confidence = if edges.is_empty() {
        0.0
    } else {
        edges.iter().map(|e| e.confidence_score).sum::<f32>() / edges.len() as f32
    };
    let stats = BeliefStats {
        nodes: nodes.len(),
        edges: edges.len(),
        avg_confidence,
        high_confidence_edges: edges.iter().filter(|e| e.confidence_score >= 0.7).count(),
        contradictions: edges
            .iter()
            .filter(|e| e.contradicts_edge_id.is_some())
            .count(),
    };

    Json(serde_json::json!({
        "ok": true,
        "stats": stats,
        "nodes": node_dtos,
        "edges": edge_dtos,
    }))
}

/// Highest-confidence path between two concepts (BFS over edges by weight).
pub async fn belief_path(
    Extension(ctx): Extension<WorkspaceContext>,
    Query(q): Query<BeliefPathQuery>,
) -> impl IntoResponse {
    let graph = &ctx.workspace.belief_graph;
    let graph = graph.read().await;
    let path = graph.find_highest_confidence_path(&q.from, &q.to).await;
    let total: f32 = path.iter().map(|e| e.confidence_score).sum();
    let dto: Vec<BeliefEdgeDto> = path
        .iter()
        .map(|e| BeliefEdgeDto {
            id: e.id.clone(),
            source: e.source.clone(),
            target: e.target.clone(),
            relation_type: e.relation_type.clone(),
            weight: e.weight,
            confidence_score: e.confidence_score,
            provenance_id: e.provenance_id.clone(),
            is_inferred: e.is_inferred,
            contradicts_edge_id: e.contradicts_edge_id.clone(),
        })
        .collect();
    Json(serde_json::json!({
        "ok": true,
        "from": q.from,
        "to": q.to,
        "path": dto,
        "total_confidence": total,
    }))
}
