//! Inbound HTTP handlers for Identity Verification Network (IVN)
//!
//! Exposes identity request creation, validator voting, request status lookup,
//! paginated request listing, and verified node discovery over REST.

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use crate::data_commons::ivn::{
    IvnConfig, ValidatorCandidate, ValidatorSelection, Verdict, VerdictEngine, VerdictStatus, Vote,
};
use crate::data_commons::types::WalletAddress;

// ---------------------------------------------------------------------------
// Module Singleton / Persistent State
// ---------------------------------------------------------------------------

/// Internal record representing an Identity Verification Request state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityRequestRecord {
    pub id: String,
    pub applicant: String,
    pub proof_hashes: Vec<String>,
    pub signature: Option<String>,
    pub assigned_validators: Vec<ValidatorCandidateDto>,
    pub votes: HashMap<String, Vote>,
    pub status: IdentityRequestStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub verdict: Option<VerdictDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityRequestStatus {
    Pending,
    Passed,
    Rejected,
    QuorumNotMet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictDto {
    pub check_count: usize,
    pub reject_count: usize,
    pub abstain_count: usize,
    pub total_votes: usize,
    pub approval_ratio: f64,
    pub effective_quorum: f64,
}

impl From<&Verdict> for VerdictDto {
    fn from(v: &Verdict) -> Self {
        Self {
            check_count: v.check_count,
            reject_count: v.reject_count,
            abstain_count: v.abstain_count,
            total_votes: v.total_votes,
            approval_ratio: v.approval_ratio,
            effective_quorum: v.effective_quorum,
        }
    }
}

/// In-memory engine store for IVN state management.
#[derive(Debug, Default)]
pub struct IvnEngineStore {
    pub requests: HashMap<String, IdentityRequestRecord>,
}

static IVN_STORE: LazyLock<RwLock<IvnEngineStore>> =
    LazyLock::new(|| RwLock::new(IvnEngineStore::default()));

/// Initialize or reset the global `IvnEngineStore` (useful for testing and setup).
pub fn init_ivn_engine(store: IvnEngineStore) {
    if let Ok(mut guard) = IVN_STORE.write() {
        *guard = store;
    } else {
        tracing::error!("IVN_STORE lock poisoned during initialization");
    }
}

fn current_ivn_store() -> std::sync::RwLockReadGuard<'static, IvnEngineStore> {
    IVN_STORE.read().expect("IVN_STORE lock poisoned")
}

fn current_ivn_store_mut() -> std::sync::RwLockWriteGuard<'static, IvnEngineStore> {
    IVN_STORE.write().expect("IVN_STORE lock poisoned")
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorCandidateDto {
    pub node_id: String,
    pub wallet: String,
    pub karma: u64,
    pub seed: String,
}

impl From<&ValidatorCandidate> for ValidatorCandidateDto {
    fn from(c: &ValidatorCandidate) -> Self {
        Self {
            node_id: c.node_id.0.clone(),
            wallet: c.wallet.0.clone(),
            karma: c.karma,
            seed: c.seed.clone(),
        }
    }
}

impl From<&ValidatorCandidateDto> for ValidatorCandidate {
    fn from(dto: &ValidatorCandidateDto) -> Self {
        Self {
            node_id: WalletAddress(dto.node_id.clone()),
            wallet: WalletAddress(dto.wallet.clone()),
            karma: dto.karma,
            seed: dto.seed.clone(),
        }
    }
}

/// DTO for creating an identity request.
#[derive(Debug, Deserialize)]
pub struct CreateIdentityRequest {
    pub applicant: String,
    pub proof_hashes: Vec<String>,
    pub signature: Option<String>,
    #[serde(default)]
    pub candidate_pool: Option<Vec<ValidatorCandidateDto>>,
    pub seed: Option<String>,
}

/// DTO for casting a validator vote.
#[derive(Debug, Deserialize)]
pub struct VoteRequest {
    pub validator_node_id: String,
    pub vote: Vote,
}

/// Pagination query parameters.
#[derive(Debug, Deserialize)]
pub struct ListRequestsQuery {
    pub page: Option<usize>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn default_validator_pool() -> Vec<ValidatorCandidate> {
    vec![
        ValidatorCandidate {
            node_id: WalletAddress("xv1_validator_node_1".into()),
            wallet: WalletAddress("xv1_wallet_val1".into()),
            karma: 500,
            seed: "val_seed_1".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_validator_node_2".into()),
            wallet: WalletAddress("xv1_wallet_val2".into()),
            karma: 600,
            seed: "val_seed_2".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_validator_node_3".into()),
            wallet: WalletAddress("xv1_wallet_val3".into()),
            karma: 700,
            seed: "val_seed_3".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_validator_node_4".into()),
            wallet: WalletAddress("xv1_wallet_val4".into()),
            karma: 800,
            seed: "val_seed_4".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_validator_node_5".into()),
            wallet: WalletAddress("xv1_wallet_val5".into()),
            karma: 900,
            seed: "val_seed_5".into(),
        },
        ValidatorCandidate {
            node_id: WalletAddress("xv1_validator_node_6".into()),
            wallet: WalletAddress("xv1_wallet_val6".into()),
            karma: 1000,
            seed: "val_seed_6".into(),
        },
    ]
}

// ---------------------------------------------------------------------------
// HTTP Handlers
// ---------------------------------------------------------------------------

/// `POST /v1/identity/request` — Create an identity verification request.
///
/// Selects 5 karma-weighted validators from the node pool, sets status to `Pending`,
/// and records the initial request.
pub async fn create_identity_request_handler(
    Json(payload): Json<CreateIdentityRequest>,
) -> impl IntoResponse {
    let now = current_timestamp();
    let request_id = format!("ivn_req_{}", ulid::Ulid::new());

    let pool = if let Some(candidates) = payload.candidate_pool {
        candidates.iter().map(ValidatorCandidate::from).collect()
    } else {
        default_validator_pool()
    };

    let exclude_seed = payload
        .seed
        .as_deref()
        .unwrap_or(payload.applicant.as_str());

    let mut rng = StdRng::seed_from_u64(now);
    let selected_validators =
        match ValidatorSelection::select_validators(&pool, exclude_seed, &mut rng) {
            Ok(v) => v,
            Err(err) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "status": "error",
                        "message": format!("Failed to select validators: {}", err)
                    })),
                )
                    .into_response();
            }
        };

    let assigned_dtos: Vec<ValidatorCandidateDto> =
        selected_validators.iter().map(ValidatorCandidateDto::from).collect();

    let record = IdentityRequestRecord {
        id: request_id.clone(),
        applicant: payload.applicant,
        proof_hashes: payload.proof_hashes,
        signature: payload.signature,
        assigned_validators: assigned_dtos,
        votes: HashMap::new(),
        status: IdentityRequestStatus::Pending,
        created_at: now,
        updated_at: now,
        verdict: None,
    };

    {
        let mut store = current_ivn_store_mut();
        store.requests.insert(request_id.clone(), record.clone());
    }

    (
        StatusCode::CREATED,
        Json(json!({
            "status": "ok",
            "request": record
        })),
    )
        .into_response()
}

/// `GET /v1/identity/request/{id}` — Query status and votes for an identity request.
pub async fn get_identity_request_handler(
    Path(id): Path<String>,
) -> impl IntoResponse {
    let store = current_ivn_store();

    if let Some(record) = store.requests.get(&id) {
        (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "request": record
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "status": "error",
                "message": format!("Identity request '{}' not found", id)
            })),
        )
            .into_response()
    }
}

/// `POST /v1/identity/{id}/vote` — Cast a validator vote for an identity request.
///
/// Enforces validator authorization (returns 403 Forbidden if validator_node_id is not
/// in the assigned validators list). Evaluates state transitions: Pending -> Passed/Rejected/QuorumNotMet.
pub async fn vote_identity_request_handler(
    Path(id): Path<String>,
    Json(payload): Json<VoteRequest>,
) -> impl IntoResponse {
    let now = current_timestamp();
    let mut store = current_ivn_store_mut();

    let record = match store.requests.get_mut(&id) {
        Some(r) => r,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "status": "error",
                    "message": format!("Identity request '{}' not found", id)
                })),
            )
                .into_response();
        }
    };

    // Check if the voting node is assigned as a validator for this request
    let is_authorized = record
        .assigned_validators
        .iter()
        .any(|v| v.node_id == payload.validator_node_id || v.wallet == payload.validator_node_id);

    if !is_authorized {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "status": "error",
                "message": format!(
                    "Unauthorized: node/wallet '{}' is not an assigned validator for request '{}'",
                    payload.validator_node_id, id
                )
            })),
        )
            .into_response();
    }

    // Insert or update vote
    record
        .votes
        .insert(payload.validator_node_id.clone(), payload.vote);
    record.updated_at = now;

    // Evaluate current votes using VerdictEngine
    let vote_vec: Vec<Vote> = record.votes.values().copied().collect();
    let config = IvnConfig::default();
    let verdict = VerdictEngine::evaluate_votes(&vote_vec, config.quorum_ratio);

    record.verdict = Some(VerdictDto::from(&verdict));

    // Update status based on verdict evaluation once all assigned validators have voted or verdict is final
    match verdict.status {
        VerdictStatus::Passed => {
            record.status = IdentityRequestStatus::Passed;
        }
        VerdictStatus::Rejected => {
            if record.votes.len() >= record.assigned_validators.len() {
                record.status = IdentityRequestStatus::Rejected;
            }
        }
        VerdictStatus::QuorumNotMet => {
            if record.votes.len() >= record.assigned_validators.len() {
                record.status = IdentityRequestStatus::QuorumNotMet;
            }
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "request_id": id,
            "validator_node_id": payload.validator_node_id,
            "vote": payload.vote,
            "request_status": record.status,
            "verdict": record.verdict
        })),
    )
        .into_response()
}

/// `GET /v1/identity/requests` — Paginated list of identity verification requests.
pub async fn list_identity_requests_handler(
    Query(query): Query<ListRequestsQuery>,
) -> impl IntoResponse {
    let store = current_ivn_store();

    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(10).max(1);

    let mut all_requests: Vec<IdentityRequestRecord> = store.requests.values().cloned().collect();
    all_requests.sort_by_key(|r| std::cmp::Reverse(r.created_at));

    let total = all_requests.len();
    let start = (page - 1) * limit;
    let paginated_requests = if start < total {
        let end = (start + limit).min(total);
        all_requests[start..end].to_vec()
    } else {
        Vec::new()
    };

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "page": page,
            "limit": limit,
            "total": total,
            "requests": paginated_requests
        })),
    )
        .into_response()
}

/// `GET /v1/identity/verified` — List of verified nodes / identity merkle-ready records.
pub async fn list_verified_nodes_handler() -> impl IntoResponse {
    let store = current_ivn_store();

    let verified_records: Vec<IdentityRequestRecord> = store
        .requests
        .values()
        .filter(|r| r.status == IdentityRequestStatus::Passed)
        .cloned()
        .collect();

    let count = verified_records.len();

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "count": count,
            "verified_nodes": verified_records
        })),
    )
        .into_response()
}
