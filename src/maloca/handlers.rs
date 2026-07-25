//! HTTP handlers for `/maloca/*`.

use super::store::MalocaStore;
use super::types::*;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

pub async fn pack(Extension(store): Extension<Arc<MalocaStore>>) -> Json<MalocaPack> {
    Json(store.pack())
}

pub async fn backlog(Extension(store): Extension<Arc<MalocaStore>>) -> Json<serde_json::Value> {
    Json(store.backlog())
}

pub async fn list_support(
    Extension(store): Extension<Arc<MalocaStore>>,
) -> Json<Vec<SupportTicket>> {
    Json(store.list_support())
}

pub async fn create_support(
    Extension(store): Extension<Arc<MalocaStore>>,
    Json(body): Json<CreateSupportBody>,
) -> Result<Json<SupportTicket>, StatusCode> {
    if body.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Json(store.create_support(body)))
}

pub async fn list_reviews(
    Extension(store): Extension<Arc<MalocaStore>>,
) -> Json<Vec<ReviewRequest>> {
    Json(store.list_reviews())
}

pub async fn list_inbox(
    Extension(store): Extension<Arc<MalocaStore>>,
) -> Json<Vec<MeshTicketOffer>> {
    Json(store.list_inbox())
}

pub async fn claim(
    Extension(store): Extension<Arc<MalocaStore>>,
    Path(id): Path<String>,
    Json(body): Json<ClaimBody>,
) -> Result<Json<MeshTicketOffer>, (StatusCode, String)> {
    store
        .claim(&id, &body.node_id)
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}

pub async fn complete(
    Extension(store): Extension<Arc<MalocaStore>>,
    Path(id): Path<String>,
) -> Result<Json<RewardReceipt>, (StatusCode, String)> {
    store
        .complete(&id)
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}

pub async fn rewards(Extension(store): Extension<Arc<MalocaStore>>) -> Json<Vec<RewardReceipt>> {
    Json(store.rewards())
}

pub async fn mesh(Extension(store): Extension<Arc<MalocaStore>>) -> Json<MeshSnapshot> {
    Json(store.mesh())
}

pub async fn list_nodes(Extension(store): Extension<Arc<MalocaStore>>) -> Json<Vec<NodeRecord>> {
    Json(store.list_nodes())
}

pub async fn params(Extension(store): Extension<Arc<MalocaStore>>) -> Json<Vec<NetworkParam>> {
    Json(store.params())
}

pub async fn list_proposals(Extension(store): Extension<Arc<MalocaStore>>) -> Json<Vec<Proposal>> {
    Json(store.list_proposals())
}

pub async fn create_proposal(
    Extension(store): Extension<Arc<MalocaStore>>,
    Json(body): Json<CreateProposalBody>,
) -> Result<Json<Proposal>, StatusCode> {
    if body.title.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Json(store.create_proposal(body)))
}

#[derive(Debug, Deserialize)]
pub struct VotesQuery {
    pub proposal_id: Option<String>,
}

pub async fn list_votes(
    Extension(store): Extension<Arc<MalocaStore>>,
    Query(q): Query<VotesQuery>,
) -> Json<Vec<Vote>> {
    Json(store.list_votes(q.proposal_id.as_deref()))
}

pub async fn cast_vote(
    Extension(store): Extension<Arc<MalocaStore>>,
    Path(id): Path<String>,
    Json(body): Json<CastVoteBody>,
) -> Result<Json<Vote>, (StatusCode, String)> {
    store.cast_vote(&id, body).map(Json).map_err(|e| {
        let msg = e.to_string();
        let status = if msg.contains("not found") {
            StatusCode::NOT_FOUND
        } else if msg.contains("vote_karma_min") || msg.contains("not active") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_REQUEST
        };
        (status, msg)
    })
}

pub async fn list_decisions(
    Extension(store): Extension<Arc<MalocaStore>>,
) -> Json<Vec<DecisionEvent>> {
    Json(store.list_decisions())
}

pub async fn list_manager_actions(
    Extension(store): Extension<Arc<MalocaStore>>,
) -> Json<Vec<ManagerAction>> {
    Json(store.list_manager_actions())
}

pub async fn manager_action(
    Extension(store): Extension<Arc<MalocaStore>>,
    Json(body): Json<ManagerActionBody>,
) -> Result<Json<ManagerAction>, (StatusCode, String)> {
    store
        .manager_action(body)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}
