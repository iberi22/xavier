//! HTTP handlers for `/maloca/*`.

use super::store::MalocaStore;
use super::types::*;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct BacklogQuery {
    pub app_id: Option<String>,
}

pub async fn pack(Extension(store): Extension<Arc<MalocaStore>>) -> Json<MalocaPack> {
    let mut pack = store.pack();
    let projects = crate::maloca::universal::scan_projects();

    let mut total = 0;
    let mut draft = 0;
    let mut zero_gaps = Vec::new();

    for p in &projects {
        for f in &p.features {
            total += 1;
            if f.status.eq_ignore_ascii_case("draft") {
                draft += 1;
            }
            if f.progress_pct == 0.0 {
                zero_gaps.push(f.id.clone());
            }
        }
    }

    pack.features_total = total as u64;
    pack.features_draft = draft as u64;
    pack.gaps_zero_symbol_modules = zero_gaps;

    Json(pack)
}

pub async fn backlog(
    Extension(store): Extension<Arc<MalocaStore>>,
    Query(q): Query<BacklogQuery>,
) -> Json<serde_json::Value> {
    let _ = store;
    let projects = crate::maloca::universal::scan_projects();
    let mut items = Vec::new();

    for p in &projects {
        if let Some(ref target_app_id) = q.app_id {
            if !p.repo_name.eq_ignore_ascii_case(target_app_id) {
                continue;
            }
        }

        for f in &p.features {
            if f.progress_pct < 100.0 {
                items.push(serde_json::json!({
                    "id": f.id.clone(),
                    "title": f.name.clone(),
                    "status": f.status.clone(),
                    "progress_pct": f.progress_pct,
                    "notes": f.notes.clone().unwrap_or_default(),
                    "repo_name": p.repo_name.clone(),
                }));
            }
        }
    }

    Json(serde_json::json!({
        "source": "xavier/src/maloca",
        "items": items,
    }))
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

pub async fn feed_status() -> Json<super::ws::FeedStatus> {
    Json(super::ws::get_feed_status())
}

// ---------------------------------------------------------------------------
// Data Node Opt-In Consent handlers
// ---------------------------------------------------------------------------

pub async fn list_consents(
    Extension(consent): Extension<Arc<super::data_node::ConsentRegistry>>,
) -> Json<Vec<super::data_node::DataNodeConsent>> {
    Json(consent.list_all())
}

pub async fn register_consent(
    Extension(consent): Extension<Arc<super::data_node::ConsentRegistry>>,
    Json(body): Json<super::data_node::ConsentBody>,
) -> Result<Json<super::data_node::DataNodeConsent>, StatusCode> {
    if body.node_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Json(consent.register(body)))
}

pub async fn get_consent(
    Extension(consent): Extension<Arc<super::data_node::ConsentRegistry>>,
    Path(node_id): Path<String>,
) -> Result<Json<super::data_node::DataNodeConsent>, (StatusCode, String)> {
    consent
        .check(&node_id)
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}

pub async fn revoke_consent(
    Extension(consent): Extension<Arc<super::data_node::ConsentRegistry>>,
    Path(node_id): Path<String>,
) -> Result<Json<super::data_node::DataNodeConsent>, (StatusCode, String)> {
    consent
        .revoke(&node_id)
        .map(Json)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
}
