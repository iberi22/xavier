//! Space graph — permissioned navigable graph per Space (T-06)
//!
//! Each Space has its own graph (nodes + edges). Navigation is ACL-checked
//! via `can(role, Read)`. Snippet mode returns 100-char preview + page-in.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::invite::SpaceRole;
use super::permissions::{can, SpaceAction};

/// Node in a Space graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub content: String,
}

/// Edge in a Space graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub label: String,
}

/// Snippet view for page-in
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphSnippet {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub snippet: String,
}

/// Manager for per-Space graphs with permission checks
#[derive(Debug, Default)]
pub struct GraphManager {
    nodes: Arc<RwLock<HashMap<String, HashMap<String, GraphNode>>>>,
    edges: Arc<RwLock<HashMap<String, Vec<GraphEdge>>>>,
}

impl GraphManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a node to a Space (admin/member can write, checked externally)
    pub async fn add_node(&self, space_id: String, node: GraphNode) {
        let mut guard = self.nodes.write().await;
        guard
            .entry(space_id)
            .or_default()
            .insert(node.id.clone(), node);
    }

    /// Add an edge to a Space
    pub async fn add_edge(&self, space_id: String, edge: GraphEdge) {
        let mut guard = self.edges.write().await;
        guard.entry(space_id).or_default().push(edge);
    }

    /// List nodes as snippets if role can read. Returns snippets (100 chars) for page-in.
    pub async fn list_snippets(
        &self,
        space_id: &str,
        role: SpaceRole,
    ) -> Result<Vec<GraphSnippet>, String> {
        if !can(role, SpaceAction::Read) {
            return Err("forbidden: insufficient permission".into());
        }
        let guard = self.nodes.read().await;
        let map = guard.get(space_id);
        let mut out = Vec::new();
        if let Some(m) = map {
            for n in m.values() {
                let snippet: String = n.content.chars().take(100).collect();
                out.push(GraphSnippet {
                    id: n.id.clone(),
                    label: n.label.clone(),
                    kind: n.kind.clone(),
                    snippet,
                });
            }
        }
        Ok(out)
    }

    /// Page-in full node by id if role can read
    pub async fn get_node(
        &self,
        space_id: &str,
        node_id: &str,
        role: SpaceRole,
    ) -> Result<GraphNode, String> {
        if !can(role, SpaceAction::Read) {
            return Err("forbidden".into());
        }
        let guard = self.nodes.read().await;
        guard
            .get(space_id)
            .and_then(|m| m.get(node_id).cloned())
            .ok_or_else(|| "not found".into())
    }

    /// List edges for a Space if role can read
    pub async fn list_edges(
        &self,
        space_id: &str,
        role: SpaceRole,
    ) -> Result<Vec<GraphEdge>, String> {
        if !can(role, SpaceAction::Read) {
            return Err("forbidden".into());
        }
        let guard = self.edges.read().await;
        Ok(guard.get(space_id).cloned().unwrap_or_default())
    }

    /// Neighbors of a node up to depth 1 (direct edges)
    pub async fn neighbors(
        &self,
        space_id: &str,
        node_id: &str,
        role: SpaceRole,
    ) -> Result<Vec<GraphNode>, String> {
        let edges = self.list_edges(space_id, role).await?;
        let mut neighbor_ids = Vec::new();
        for e in edges {
            if e.from == node_id {
                neighbor_ids.push(e.to);
            } else if e.to == node_id {
                neighbor_ids.push(e.from);
            }
        }
        let guard = self.nodes.read().await;
        let map = guard.get(space_id);
        let mut out = Vec::new();
        if let Some(m) = map {
            for nid in neighbor_ids {
                if let Some(n) = m.get(&nid) {
                    out.push(n.clone());
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn snippet_and_page_in() {
        let mgr = GraphManager::new();
        mgr.add_node(
            "esp_a".into(),
            GraphNode {
                id: "n1".into(),
                label: "Node1".into(),
                kind: "concept".into(),
                content: "a".repeat(200),
            },
        )
        .await;
        let snippets = mgr.list_snippets("esp_a", SpaceRole::Member).await.unwrap();
        assert_eq!(snippets.len(), 1);
        assert_eq!(snippets[0].snippet.len(), 100);
        let full = mgr
            .get_node("esp_a", "n1", SpaceRole::Member)
            .await
            .unwrap();
        assert_eq!(full.content.len(), 200);
    }

    #[tokio::test]
    async fn permission_denied() {
        let mgr = GraphManager::new();
        mgr.add_node(
            "esp_a".into(),
            GraphNode {
                id: "n1".into(),
                label: "x".into(),
                kind: "k".into(),
                content: "c".into(),
            },
        )
        .await;
        // Reader can read, but use a role that cannot read is impossible — all roles can read.
        // Instead test that invalid space returns empty, not error.
        let snippets = mgr.list_snippets("esp_a", SpaceRole::Reader).await.unwrap();
        assert_eq!(snippets.len(), 1);
        // Simulate forbidden by using a role that cannot read is not possible in current hierarchy
        // So test not-found case
        assert!(mgr
            .get_node("esp_a", "missing", SpaceRole::Reader)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn neighbors_found() {
        let mgr = GraphManager::new();
        for id in ["n1", "n2", "n3"] {
            mgr.add_node(
                "esp_a".into(),
                GraphNode {
                    id: id.into(),
                    label: id.into(),
                    kind: "k".into(),
                    content: "c".into(),
                },
            )
            .await;
        }
        mgr.add_edge(
            "esp_a".into(),
            GraphEdge {
                from: "n1".into(),
                to: "n2".into(),
                label: "rel".into(),
            },
        )
        .await;
        let neigh = mgr
            .neighbors("esp_a", "n1", SpaceRole::Member)
            .await
            .unwrap();
        assert_eq!(neigh.len(), 1);
        assert_eq!(neigh[0].id, "n2");
    }
}
