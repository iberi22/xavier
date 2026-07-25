//! Knowledge graph API endpoints
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use axum::{
    extract::{Extension, Path, Query},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::{
    memory::entity_graph::{EntityNeighbors, EntityRecord, EntityRelationRecord, GraphDirection},
    workspace::WorkspaceContext,
};

#[derive(Debug, Deserialize)]
pub struct GraphEntityQuery {
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub relation_types: Option<Vec<String>>,
    #[serde(default)]
    pub direction: Option<GraphDirection>,
}

#[derive(Debug, Deserialize)]
pub struct GraphRelationsQuery {
    #[serde(default)]
    pub entity_id: Option<String>,
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,
    #[serde(default)]
    pub relation_types: Option<Vec<String>>,
    #[serde(default)]
    pub direction: Option<GraphDirection>,
}

#[derive(Debug, Deserialize)]
pub struct GraphListQuery {
    pub q: Option<String>,
    pub entity_type: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct GraphViewQuery {
    pub limit_nodes: Option<usize>,
    pub min_weight: Option<f32>,
    pub entity_id: Option<String>,
    pub max_depth: Option<usize>,
    pub entity_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub description: Option<String>,
    pub trust_score: f32,
    pub memory_count: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewLink {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub weight: f32,
    pub confidence_score: f32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewStats {
    pub entities: usize,
    pub relations: usize,
    pub shown_nodes: usize,
    pub shown_links: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphViewResponse {
    pub status: String,
    pub layer: String,
    pub truncated: bool,
    pub nodes: Vec<GraphViewNode>,
    pub links: Vec<GraphViewLink>,
    pub stats: GraphViewStats,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GraphListResponse {
    pub status: String,
    pub total: usize,
    pub count: usize,
    pub entities: Vec<EntityRecord>,
}

impl From<EntityRecord> for GraphViewNode {
    fn from(record: EntityRecord) -> Self {
        Self {
            id: record.id,
            label: record.name,
            kind: record.entity_type.as_str().to_string(),
            description: record.description,
            trust_score: record.trust_score,
            memory_count: record.memory_count,
        }
    }
}

impl From<EntityRelationRecord> for GraphViewLink {
    fn from(record: EntityRelationRecord) -> Self {
        Self {
            source: record.source,
            target: record.target,
            relation: record.relation_type,
            weight: record.weight,
            confidence_score: record.confidence_score,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct GraphEntityResponse {
    pub status: String,
    pub entity: crate::memory::entity_graph::EntityRecord,
    pub incoming: Vec<crate::memory::entity_graph::EntityRelationRecord>,
    pub outgoing: Vec<crate::memory::entity_graph::EntityRelationRecord>,
    pub traversal: Vec<crate::memory::entity_graph::TraversalStep>,
}

#[derive(Debug, Serialize)]
pub struct GraphRelationsResponse {
    pub status: String,
    pub entity_id: Option<String>,
    pub direction: GraphDirection,
    pub max_depth: usize,
    pub total_relations: usize,
    pub relations: Vec<crate::memory::entity_graph::EntityRelationRecord>,
    pub traversal: Vec<crate::memory::entity_graph::TraversalStep>,
}

fn default_max_depth() -> usize {
    2
}

/// Memory graph entity.
pub async fn memory_graph_entity(
    Extension(workspace): Extension<WorkspaceContext>,
    Path(entity_id): Path<String>,
    Query(query): Query<GraphEntityQuery>,
) -> impl IntoResponse {
    let direction = query.direction.unwrap_or_default();
    let relation_types = query.relation_types.as_deref();
    match workspace
        .workspace
        .entity_graph
        .entity_neighbors(&entity_id, query.max_depth, relation_types, direction)
        .await
    {
        Ok(EntityNeighbors {
            entity,
            incoming,
            outgoing,
            traversal,
        }) => Json(GraphEntityResponse {
            status: "ok".to_string(),
            entity,
            incoming,
            outgoing,
            traversal,
        })
        .into_response(),
        Err(error) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "error",
                "message": error.to_string(),
                "entity_id": entity_id,
            })),
        )
            .into_response(),
    }
}

/// Memory graph relations.
pub async fn memory_graph_relations(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(query): Query<GraphRelationsQuery>,
) -> impl IntoResponse {
    let direction = query.direction.unwrap_or(GraphDirection::Both);
    let relation_types = query.relation_types.as_deref();

    if let Some(entity_id) = query.entity_id {
        match workspace
            .workspace
            .entity_graph
            .relations_for_entity(&entity_id, query.max_depth, relation_types, direction)
            .await
        {
            Ok(view) => Json(GraphRelationsResponse {
                status: "ok".to_string(),
                entity_id: view.entity_id,
                direction: view.direction,
                max_depth: view.max_depth,
                total_relations: view.total_relations,
                relations: view.relations,
                traversal: view.traversal,
            })
            .into_response(),
            Err(error) => (
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "status": "error",
                    "message": error.to_string(),
                    "entity_id": entity_id,
                })),
            )
                .into_response(),
        }
    } else {
        let relations = workspace.workspace.entity_graph.all_relations().await;
        Json(GraphRelationsResponse {
            status: "ok".to_string(),
            entity_id: None,
            direction,
            max_depth: query.max_depth,
            total_relations: relations.len(),
            relations,
            traversal: Vec::new(),
        })
        .into_response()
    }
}

/// Memory graph list entities.
pub async fn memory_graph_list_entities(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(query): Query<GraphListQuery>,
) -> impl IntoResponse {
    let mut entities = workspace.workspace.entity_graph.all_entities().await;

    // Filter by query substring (case-insensitive) on name or normalized_name
    if let Some(ref search) = query.q {
        let search_lower = search.to_lowercase();
        entities.retain(|e| {
            e.name.to_lowercase().contains(&search_lower)
                || e.normalized_name.to_lowercase().contains(&search_lower)
        });
    }

    // Filter by entity type (comma-separated list or single type)
    if let Some(ref type_str) = query.entity_type {
        let allowed_types: HashSet<String> = type_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        entities.retain(|e| allowed_types.contains(&e.entity_type.as_str().to_lowercase()));
    }

    // Sort entities by trust_score desc, then name (stable order)
    entities.sort_by(|a, b| {
        b.trust_score
            .total_cmp(&a.trust_score)
            .then_with(|| a.name.cmp(&b.name))
    });

    let total = entities.len();
    let limit = query.limit.unwrap_or(500).min(2000);
    let offset = query.offset.unwrap_or(0);

    let paginated_entities: Vec<EntityRecord> =
        entities.into_iter().skip(offset).take(limit).collect();

    let count = paginated_entities.len();

    Json(GraphListResponse {
        status: "ok".to_string(),
        total,
        count,
        entities: paginated_entities,
    })
    .into_response()
}

/// Memory graph view.
pub async fn memory_graph_view(
    Extension(workspace): Extension<WorkspaceContext>,
    Query(query): Query<GraphViewQuery>,
) -> impl IntoResponse {
    let limit_nodes = query.limit_nodes.unwrap_or(500).min(2000);
    let min_weight = query.min_weight.unwrap_or(0.0);

    let mut nodes: Vec<EntityRecord> = if let Some(ref entity_id) = query.entity_id {
        // Ego-graph case
        let max_depth = query.max_depth.unwrap_or(2);
        let neighbors = match workspace
            .workspace
            .entity_graph
            .entity_neighbors(entity_id, max_depth, None, GraphDirection::Both)
            .await
        {
            Ok(n) => n,
            Err(error) => {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": error.to_string(),
                        "entity_id": entity_id,
                    })),
                )
                    .into_response();
            }
        };

        // Collect all unique node IDs in ego-graph
        let mut ego_node_ids = HashSet::new();
        ego_node_ids.insert(neighbors.entity.id.clone());

        for r in &neighbors.incoming {
            ego_node_ids.insert(r.source.clone());
            ego_node_ids.insert(r.target.clone());
        }
        for r in &neighbors.outgoing {
            ego_node_ids.insert(r.source.clone());
            ego_node_ids.insert(r.target.clone());
        }
        for t in &neighbors.traversal {
            ego_node_ids.insert(t.from.clone());
            ego_node_ids.insert(t.to.clone());
            for p in &t.path {
                ego_node_ids.insert(p.clone());
            }
        }

        let all_entities = workspace.workspace.entity_graph.all_entities().await;
        all_entities
            .into_iter()
            .filter(|e| ego_node_ids.contains(&e.id))
            .collect()
    } else {
        // All entities case
        workspace.workspace.entity_graph.all_entities().await
    };

    // Filter by entity type if specified
    if let Some(ref type_str) = query.entity_type {
        let allowed_types: HashSet<String> = type_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        nodes.retain(|e| allowed_types.contains(&e.entity_type.as_str().to_lowercase()));
    }

    // Sort nodes stably: trust_score desc, then name asc
    nodes.sort_by(|a, b| {
        b.trust_score
            .total_cmp(&a.trust_score)
            .then_with(|| a.name.cmp(&b.name))
    });

    let total_nodes = nodes.len();
    let truncated = total_nodes > limit_nodes;

    // Separate before and after truncation sets for exact statistics
    let all_matching_node_ids: HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();

    if truncated {
        nodes.truncate(limit_nodes);
    }

    let truncated_node_ids: HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();

    // Construct links / relations
    let all_relations = workspace.workspace.entity_graph.all_relations().await;

    // Calculate untruncated relations statistics (matching weight and matching untruncated nodes)
    let total_relations = all_relations
        .iter()
        .filter(|r| {
            all_matching_node_ids.contains(&r.source)
                && all_matching_node_ids.contains(&r.target)
                && r.weight >= min_weight
        })
        .count();

    // Filter final links to only those connecting the remaining (truncated) nodes
    let mut links: Vec<EntityRelationRecord> = all_relations
        .into_iter()
        .filter(|r| {
            truncated_node_ids.contains(&r.source)
                && truncated_node_ids.contains(&r.target)
                && r.weight >= min_weight
        })
        .collect();

    // Sort links stably for reliable UI representation
    links.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.relation_type.cmp(&b.relation_type))
    });

    let view_nodes: Vec<GraphViewNode> = nodes.into_iter().map(GraphViewNode::from).collect();
    let view_links: Vec<GraphViewLink> = links.into_iter().map(GraphViewLink::from).collect();

    let shown_nodes = view_nodes.len();
    let shown_links = view_links.len();

    Json(GraphViewResponse {
        status: "ok".to_string(),
        layer: "memory".to_string(),
        truncated,
        nodes: view_nodes,
        links: view_links,
        stats: GraphViewStats {
            entities: total_nodes,
            relations: total_relations,
            shown_nodes,
            shown_links,
        },
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::RuntimeConfig;
    use crate::memory::store::MemoryBackend;
    use crate::workspace::config::PlanTier;
    use crate::workspace::state::WorkspaceState;
    use crate::workspace::EmbeddingProviderMode;
    use crate::workspace::SyncPolicy;
    use crate::workspace::WorkspaceConfig;
    use axum::response::IntoResponse;
    use std::sync::Arc;
    use ulid::Ulid;

    async fn make_test_context() -> WorkspaceContext {
        let unique_id = Ulid::new().to_string();
        let root = std::env::temp_dir().join(format!("xavier-api-graph-test-{}", unique_id));
        let config = WorkspaceConfig {
            id: format!("test-graph-{}", unique_id),
            token: "test-token".to_string(),
            plan: PlanTier::Personal,
            memory_backend: MemoryBackend::Memory,
            storage_limit_bytes: Some(10 * 1024 * 1024),
            request_limit: Some(1000),
            request_unit_limit: Some(1000),
            embedding_provider_mode: EmbeddingProviderMode::BringYourOwn,
            managed_google_embeddings: false,
            sync_policy: SyncPolicy::LocalOnly,
            dedup: crate::settings::types::DedupSettings::default(),
        };
        let workspace = WorkspaceState::new(config, RuntimeConfig::default(), root)
            .await
            .expect("should create workspace state");
        WorkspaceContext {
            workspace_id: workspace.config().id.clone(),
            workspace: Arc::new(workspace),
        }
    }

    #[tokio::test]
    async fn test_empty_graph_projection() {
        let ctx = make_test_context().await;

        // Test list entities on empty graph
        let list_query = GraphListQuery {
            q: None,
            entity_type: None,
            limit: None,
            offset: None,
        };
        let response = memory_graph_list_entities(Extension(ctx.clone()), Query(list_query))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        // Test view projection on empty graph
        let view_query = GraphViewQuery {
            limit_nodes: None,
            min_weight: None,
            entity_id: None,
            max_depth: None,
            entity_type: None,
        };
        let response = memory_graph_view(Extension(ctx), Query(view_query))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_missing_entity_returns_404() {
        let ctx = make_test_context().await;

        let entity_query = GraphEntityQuery {
            max_depth: 2,
            relation_types: None,
            direction: None,
        };

        let response = memory_graph_entity(
            Extension(ctx.clone()),
            Path("non-existent-entity-id".to_string()),
            Query(entity_query),
        )
        .await
        .into_response();

        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);

        // Also test view with missing entity id returns 404
        let view_query = GraphViewQuery {
            limit_nodes: None,
            min_weight: None,
            entity_id: Some("non-existent-entity-id".to_string()),
            max_depth: None,
            entity_type: None,
        };
        let response = memory_graph_view(Extension(ctx), Query(view_query))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_graph_projection_with_data() {
        let ctx = make_test_context().await;

        ctx.workspace
            .entity_graph
            .upsert_memory(
                "mem-1",
                "BELA works at SWAL and knows Leonardo in Bogota.",
                None,
            )
            .await
            .unwrap();

        // 1. List entities test
        let list_query = GraphListQuery {
            q: Some("bela".to_string()),
            entity_type: None,
            limit: Some(10),
            offset: Some(0),
        };
        let response = memory_graph_list_entities(Extension(ctx.clone()), Query(list_query))
            .await
            .into_response();
        assert_eq!(response.status(), axum::http::StatusCode::OK);

        // 2. View projection test (limit truncates and sets truncated flag)
        let view_query = GraphViewQuery {
            limit_nodes: Some(2), // We extracted Bela, SWAL, Leonardo, Bogota (4 entities)
            min_weight: Some(0.0),
            entity_id: None,
            max_depth: None,
            entity_type: None,
        };

        let response = memory_graph_view(Extension(ctx.clone()), Query(view_query)).await;
        let body_bytes = axum::body::to_bytes(response.into_response().into_body(), 1024 * 1024)
            .await
            .unwrap();
        let view_res: GraphViewResponse = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(view_res.status, "ok");
        assert!(view_res.truncated);
        assert_eq!(view_res.nodes.len(), 2);
        assert_eq!(view_res.stats.entities, 4); // total matching entities before truncation is 4
        assert!(view_res.stats.shown_nodes <= 2);

        // 3. View projection with entity_type filter test
        let view_query_filtered = GraphViewQuery {
            limit_nodes: Some(10),
            min_weight: Some(0.0),
            entity_id: None,
            max_depth: None,
            entity_type: Some("person".to_string()),
        };
        let response_filtered = memory_graph_view(Extension(ctx), Query(view_query_filtered)).await;
        let body_bytes_filtered =
            axum::body::to_bytes(response_filtered.into_response().into_body(), 1024 * 1024)
                .await
                .unwrap();
        let view_res_filtered: GraphViewResponse =
            serde_json::from_slice(&body_bytes_filtered).unwrap();

        assert_eq!(view_res_filtered.status, "ok");
        assert!(!view_res_filtered.truncated);
        // Only "person" type nodes should remain
        for node in &view_res_filtered.nodes {
            assert_eq!(node.kind, "person");
        }
    }
}
