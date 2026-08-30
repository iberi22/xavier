//! REST routes for DAO democratic 1-node-1-vote proposal and quorum management under `/v1/mesh/dao/*`.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Ballot options for 1-node-1-vote governance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum VoteOption {
    For,
    Against,
    Abstain,
}

/// Request payload to create a new DAO governance proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProposalRequest {
    pub title: String,
    pub description: String,
    pub category: String,
    pub required_endorsement: String,
}

/// Request payload for submitting a ballot on a proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteRequest {
    pub node_id: String,
    pub ballot: VoteOption,
    pub endorsement_badge: Option<String>,
}

/// Representation of a node's endorsement badge (avales).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEndorsement {
    pub node_id: String,
    pub badge: String,
}

/// In-memory DAO proposal state tracking tallies, voters, and quorum status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoProposal {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub required_endorsement: String,
    pub for_votes: u64,
    pub against_votes: u64,
    pub abstain_votes: u64,
    pub total_votes: u64,
    pub quorum_reached: bool,
    pub is_active: bool,
    pub voters: HashSet<String>,
}

/// Public API response for a proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalResponse {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub required_endorsement: String,
    pub for_votes: u64,
    pub against_votes: u64,
    pub abstain_votes: u64,
    pub total_votes: u64,
    pub quorum_reached: bool,
    pub is_active: bool,
}

impl From<&DaoProposal> for ProposalResponse {
    fn from(p: &DaoProposal) -> Self {
        Self {
            id: p.id.clone(),
            title: p.title.clone(),
            description: p.description.clone(),
            category: p.category.clone(),
            required_endorsement: p.required_endorsement.clone(),
            for_votes: p.for_votes,
            against_votes: p.against_votes,
            abstain_votes: p.abstain_votes,
            total_votes: p.total_votes,
            quorum_reached: p.quorum_reached,
            is_active: p.is_active,
        }
    }
}

/// DAO Governance state holding stored proposals, node endorsements, and quorum thresholds.
#[derive(Clone)]
pub struct MeshGovernanceState {
    pub inner: Arc<RwLock<MeshGovernanceStore>>,
}

pub struct MeshGovernanceStore {
    pub proposals: HashMap<String, DaoProposal>,
    pub node_endorsements: HashMap<String, HashSet<String>>,
    pub minimum_quorum: u64,
    pub next_proposal_id: u64,
}

impl Default for MeshGovernanceState {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshGovernanceState {
    /// Creates a new `MeshGovernanceState` instance with default minimum quorum threshold of 5.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(MeshGovernanceStore {
                proposals: HashMap::new(),
                node_endorsements: HashMap::new(),
                minimum_quorum: 5,
                next_proposal_id: 1,
            })),
        }
    }

    /// Helper to register a node endorsement badge (avales).
    pub fn register_node_endorsement(&self, node_id: &str, badge: &str) {
        let mut store = self.inner.write().unwrap();
        store
            .node_endorsements
            .entry(node_id.to_string())
            .or_default()
            .insert(badge.to_string());
    }
}

/// Builds the Axum Router for mesh DAO governance routes.
pub fn router(state: MeshGovernanceState) -> Router {
    Router::new()
        .route("/v1/mesh/dao/proposals", post(create_proposal_handler))
        .route("/v1/mesh/dao/proposals", get(list_proposals_handler))
        .route(
            "/v1/mesh/dao/proposals/{id}/vote",
            post(cast_vote_handler),
        )
        .route(
            "/v1/mesh/dao/endorsements",
            get(list_endorsements_handler).post(register_endorsement_handler),
        )
        .with_state(state)
}

/// POST /v1/mesh/dao/proposals
/// Creates a new governance proposal.
pub async fn create_proposal_handler(
    State(state): State<MeshGovernanceState>,
    Json(payload): Json<CreateProposalRequest>,
) -> impl IntoResponse {
    if payload.title.trim().is_empty() || payload.description.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Title and description cannot be empty"
            })),
        )
            .into_response();
    }

    let mut store = state.inner.write().unwrap();
    let proposal_id = format!("prop_{}", store.next_proposal_id);
    store.next_proposal_id += 1;

    let proposal = DaoProposal {
        id: proposal_id.clone(),
        title: payload.title,
        description: payload.description,
        category: payload.category,
        required_endorsement: payload.required_endorsement,
        for_votes: 0,
        against_votes: 0,
        abstain_votes: 0,
        total_votes: 0,
        quorum_reached: false,
        is_active: true,
        voters: HashSet::new(),
    };

    let response: ProposalResponse = (&proposal).into();
    store.proposals.insert(proposal_id, proposal);

    (StatusCode::CREATED, Json(response)).into_response()
}

/// GET /v1/mesh/dao/proposals
/// Lists active proposals with vote tallies and quorum status.
pub async fn list_proposals_handler(
    State(state): State<MeshGovernanceState>,
) -> impl IntoResponse {
    let store = state.inner.read().unwrap();
    let active_proposals: Vec<ProposalResponse> = store
        .proposals
        .values()
        .filter(|p| p.is_active)
        .map(ProposalResponse::from)
        .collect();

    Json(active_proposals).into_response()
}

/// POST /v1/mesh/dao/proposals/{id}/vote
/// Submits ballot (For, Against, Abstain) enforcing 1 vote per node ID per proposal and endorsement badge verification.
pub async fn cast_vote_handler(
    State(state): State<MeshGovernanceState>,
    Path(id): Path<String>,
    Json(payload): Json<VoteRequest>,
) -> impl IntoResponse {
    let mut store = state.inner.write().unwrap();
    let min_quorum = store.minimum_quorum;

    let node_id = payload.node_id.trim().to_string();
    if node_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "node_id cannot be empty"
            })),
        )
            .into_response();
    }

    // Endorsement lookup before mutable borrow of proposal
    let node_badges = store
        .node_endorsements
        .get(&node_id)
        .cloned()
        .unwrap_or_default();

    let proposal = match store.proposals.get_mut(&id) {
        Some(p) => p,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "status": "error",
                    "message": "Proposal not found"
                })),
            )
                .into_response();
        }
    };

    if !proposal.is_active {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Proposal is not active"
            })),
        )
            .into_response();
    }

    // Anti-Hallucination Guard: Enforce 1 vote per node ID per proposal
    if proposal.voters.contains(&node_id) {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "status": "error",
                "message": "Node has already voted on this proposal"
            })),
        )
            .into_response();
    }

    // Endorsement Verification (avales)
    let required = proposal.required_endorsement.trim();
    if !required.is_empty() && required != "none" {
        let payload_badge = payload.endorsement_badge.as_deref().unwrap_or("");
        let has_endorsement = (payload_badge == required) || node_badges.contains(required);

        if !has_endorsement {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Missing required endorsement badge: {}", required)
                })),
            )
                .into_response();
        }
    }

    // Register 1-node-1-vote ballot
    proposal.voters.insert(node_id);
    match payload.ballot {
        VoteOption::For => proposal.for_votes += 1,
        VoteOption::Against => proposal.against_votes += 1,
        VoteOption::Abstain => proposal.abstain_votes += 1,
    }
    proposal.total_votes = proposal.for_votes + proposal.against_votes + proposal.abstain_votes;

    if proposal.total_votes >= min_quorum {
        proposal.quorum_reached = true;
    }

    let response: ProposalResponse = (&*proposal).into();
    (StatusCode::OK, Json(response)).into_response()
}

/// GET /v1/mesh/dao/endorsements
/// Lists node endorsements (avales).
pub async fn list_endorsements_handler(
    State(state): State<MeshGovernanceState>,
) -> impl IntoResponse {
    let store = state.inner.read().unwrap();
    let endorsements: HashMap<String, Vec<String>> = store
        .node_endorsements
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
        .collect();

    Json(endorsements).into_response()
}

/// POST /v1/mesh/dao/endorsements
/// Registers a node endorsement badge (avales).
pub async fn register_endorsement_handler(
    State(state): State<MeshGovernanceState>,
    Json(payload): Json<NodeEndorsement>,
) -> impl IntoResponse {
    if payload.node_id.trim().is_empty() || payload.badge.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "node_id and badge cannot be empty"
            })),
        )
            .into_response();
    }

    state.register_node_endorsement(&payload.node_id, &payload.badge);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "status": "ok",
            "message": "Endorsement registered successfully"
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn test_mesh_dao_governance_voting() {
        let state = MeshGovernanceState::new();
        let app = router(state.clone());

        // 1. Register node endorsement badge for test nodes
        state.register_node_endorsement("node_alpha", "core_maintainer");
        state.register_node_endorsement("node_beta", "core_maintainer");
        state.register_node_endorsement("node_gamma", "core_maintainer");
        state.register_node_endorsement("node_delta", "core_maintainer");
        state.register_node_endorsement("node_epsilon", "core_maintainer");

        // 2. Create proposal with required endorsement "core_maintainer"
        let create_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/dao/proposals")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "title": "Upgrade Mesh Protocol",
                    "description": "Proposal to mandate 1-node-1-vote democratic consensus",
                    "category": "protocol_upgrade",
                    "required_endorsement": "core_maintainer"
                })
                .to_string(),
            ))
            .unwrap();

        let create_res = app.clone().oneshot(create_req).await.unwrap();
        assert_eq!(create_res.status(), StatusCode::CREATED);

        let body_bytes = to_bytes(create_res.into_body(), usize::MAX).await.unwrap();
        let prop_res: ProposalResponse = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(prop_res.id, "prop_1");
        assert_eq!(prop_res.title, "Upgrade Mesh Protocol");
        assert_eq!(prop_res.required_endorsement, "core_maintainer");
        assert_eq!(prop_res.total_votes, 0);
        assert!(!prop_res.quorum_reached);

        // 3. Test node without required endorsement badge (should be forbidden)
        let unendorsed_vote_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/dao/proposals/prop_1/vote")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "node_unauthorized",
                    "ballot": "For"
                })
                .to_string(),
            ))
            .unwrap();

        let unendorsed_res = app.clone().oneshot(unendorsed_vote_req).await.unwrap();
        assert_eq!(unendorsed_res.status(), StatusCode::FORBIDDEN);

        // 4. Cast votes from valid endorsed nodes
        let nodes = vec![
            ("node_alpha", VoteOption::For),
            ("node_beta", VoteOption::For),
            ("node_gamma", VoteOption::Against),
            ("node_delta", VoteOption::Abstain),
        ];

        for (node_id, ballot) in nodes {
            let vote_req = Request::builder()
                .method("POST")
                .uri("/v1/mesh/dao/proposals/prop_1/vote")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "node_id": node_id,
                        "ballot": ballot
                    })
                    .to_string(),
                ))
                .unwrap();

            let vote_res = app.clone().oneshot(vote_req).await.unwrap();
            assert_eq!(vote_res.status(), StatusCode::OK);
        }

        // 5. Test Anti-Hallucination Guard: enforce 1 vote per node ID per proposal
        let duplicate_vote_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/dao/proposals/prop_1/vote")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "node_alpha",
                    "ballot": "Against"
                })
                .to_string(),
            ))
            .unwrap();

        let duplicate_res = app.clone().oneshot(duplicate_vote_req).await.unwrap();
        assert_eq!(duplicate_res.status(), StatusCode::CONFLICT);

        // Check proposal state after 4 votes (quorum threshold is 5, so quorum not reached yet)
        let list_req = Request::builder()
            .method("GET")
            .uri("/v1/mesh/dao/proposals")
            .body(Body::empty())
            .unwrap();

        let list_res = app.clone().oneshot(list_req).await.unwrap();
        assert_eq!(list_res.status(), StatusCode::OK);

        let list_bytes = to_bytes(list_res.into_body(), usize::MAX).await.unwrap();
        let list_props: Vec<ProposalResponse> = serde_json::from_slice(&list_bytes).unwrap();
        assert_eq!(list_props.len(), 1);
        assert_eq!(list_props[0].for_votes, 2);
        assert_eq!(list_props[0].against_votes, 1);
        assert_eq!(list_props[0].abstain_votes, 1);
        assert_eq!(list_props[0].total_votes, 4);
        assert!(!list_props[0].quorum_reached);

        // 6. Cast 5th vote reaching minimum quorum of 5
        let fifth_vote_req = Request::builder()
            .method("POST")
            .uri("/v1/mesh/dao/proposals/prop_1/vote")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({
                    "node_id": "node_epsilon",
                    "ballot": "For"
                })
                .to_string(),
            ))
            .unwrap();

        let fifth_res = app.clone().oneshot(fifth_vote_req).await.unwrap();
        assert_eq!(fifth_res.status(), StatusCode::OK);

        let fifth_bytes = to_bytes(fifth_res.into_body(), usize::MAX).await.unwrap();
        let final_prop: ProposalResponse = serde_json::from_slice(&fifth_bytes).unwrap();
        assert_eq!(final_prop.for_votes, 3);
        assert_eq!(final_prop.total_votes, 5);
        assert!(final_prop.quorum_reached);
    }
}
