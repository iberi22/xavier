//! Inbound HTTP handlers for Identity Verification Network (IVN)
//!
//! Exposes identity request creation, validator voting, request status lookup,
//! paginated request listing, and verified node discovery over REST.

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, LazyLock, RwLock};

use super::error_response;
use crate::data_commons::ivn::{
    apply_rewards, apply_sanctions, IvnConfig, IvnError, KarmaEngine, ValidatorCandidate,
    ValidatorSelection, Verdict, VerdictEngine, VerdictStatus, Vote,
};
use crate::data_commons::reputation::{EigenTrustEngine, ReputationConfig};
use crate::data_commons::types::WalletAddress;
use crate::security::auth::{Claims, Permission};

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

/// Lazily loaded IVN state with private JSON snapshots for identity requests.
#[derive(Debug)]
pub struct IvnEngineStore {
    pub requests: HashMap<String, IdentityRequestRecord>,
    pub karma_engine: KarmaEngine,
    load_error: Option<String>,
}

impl IvnEngineStore {
    /// Reload identity requests and their votes from the IVN root.
    pub fn load() -> Result<Self, String> {
        Ok(Self {
            requests: crate::data_commons::ivn_storage::load_json(
                &crate::data_commons::ivn_storage::ivn_root()?.join("requests.json"),
            )?,
            karma_engine: KarmaEngine::new(),
            load_error: None,
        })
    }

    fn save_requests(
        &self,
        requests: &HashMap<String, IdentityRequestRecord>,
    ) -> Result<(), String> {
        if let Some(err) = &self.load_error {
            return Err(err.clone());
        }
        crate::data_commons::ivn_storage::save_json(
            &crate::data_commons::ivn_storage::ivn_root()?.join("requests.json"),
            requests,
        )
    }
}

impl Default for IvnEngineStore {
    fn default() -> Self {
        Self::load().unwrap_or_else(|err| {
            tracing::error!(error = %err, "Failed to load IVN requests");
            Self {
                requests: HashMap::new(),
                karma_engine: KarmaEngine::new(),
                load_error: Some(err),
            }
        })
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
}

impl From<&ValidatorCandidate> for ValidatorCandidateDto {
    fn from(c: &ValidatorCandidate) -> Self {
        Self {
            node_id: c.node_id.0.clone(),
            wallet: c.wallet.0.clone(),
            karma: c.karma,
            seed: c.seed.clone(),
            operator_id: None,
            ip_address: None,
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
    #[serde(default)]
    pub submitter_node_id: Option<String>,
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

fn default_validator_pool() -> Vec<ValidatorCandidateDto> {
    vec![
        ValidatorCandidateDto {
            node_id: "xv1_validator_node_1".into(),
            wallet: "xv1_wallet_val1".into(),
            karma: 500,
            seed: "val_seed_1".into(),
            operator_id: Some("op_1".into()),
            ip_address: Some("192.168.1.1".into()),
        },
        ValidatorCandidateDto {
            node_id: "xv1_validator_node_2".into(),
            wallet: "xv1_wallet_val2".into(),
            karma: 600,
            seed: "val_seed_2".into(),
            operator_id: Some("op_2".into()),
            ip_address: Some("192.168.2.1".into()),
        },
        ValidatorCandidateDto {
            node_id: "xv1_validator_node_3".into(),
            wallet: "xv1_wallet_val3".into(),
            karma: 700,
            seed: "val_seed_3".into(),
            operator_id: Some("op_3".into()),
            ip_address: Some("192.168.3.1".into()),
        },
        ValidatorCandidateDto {
            node_id: "xv1_validator_node_4".into(),
            wallet: "xv1_wallet_val4".into(),
            karma: 800,
            seed: "val_seed_4".into(),
            operator_id: Some("op_4".into()),
            ip_address: Some("192.168.4.1".into()),
        },
        ValidatorCandidateDto {
            node_id: "xv1_validator_node_5".into(),
            wallet: "xv1_wallet_val5".into(),
            karma: 900,
            seed: "val_seed_5".into(),
            operator_id: Some("op_5".into()),
            ip_address: Some("192.168.5.1".into()),
        },
        ValidatorCandidateDto {
            node_id: "xv1_validator_node_6".into(),
            wallet: "xv1_wallet_val6".into(),
            karma: 1000,
            seed: "val_seed_6".into(),
            operator_id: Some("op_6".into()),
            ip_address: Some("192.168.6.1".into()),
        },
    ]
}

/// Safely parse IPv4 or IPv6 string into a subnet prefix (/24 for IPv4, /64 for IPv6).
pub fn extract_ip_subnet(ip_str: &str) -> Option<String> {
    if let Ok(ip) = std::net::IpAddr::from_str(ip_str) {
        match ip {
            std::net::IpAddr::V4(v4) => {
                let octets = v4.octets();
                Some(format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]))
            }
            std::net::IpAddr::V6(v6) => {
                let segs = v6.segments();
                Some(format!(
                    "{:x}:{:x}:{:x}:{:x}::/64",
                    segs[0], segs[1], segs[2], segs[3]
                ))
            }
        }
    } else {
        None
    }
}

/// Select validators with Sybil resistance: self-exclusion, operator identity cap, and IP subnet cap.
pub fn select_validators_with_diversity<R: Rng + ?Sized>(
    node_pool: &[ValidatorCandidateDto],
    submitter_node_id: &str,
    applicant: &str,
    exclude_seed: &str,
    rng: &mut R,
) -> Result<Vec<ValidatorCandidateDto>, IvnError> {
    let config = IvnConfig::default();

    // Filter candidates for self-exclusion, minimum karma, and active exclusion windows
    let mut eligible: Vec<ValidatorCandidateDto> = node_pool
        .iter()
        .filter(|c| {
            c.node_id != submitter_node_id
                && c.wallet != submitter_node_id
                && c.node_id != applicant
                && c.wallet != applicant
                && c.seed != exclude_seed
                && c.karma >= config.karma_min_validator
                && !crate::data_commons::ivn::is_excluded(&WalletAddress(c.node_id.clone()))
                && !crate::data_commons::ivn::is_excluded(&WalletAddress(c.wallet.clone()))
        })
        .cloned()
        .collect();

    if eligible.is_empty() {
        return Err(IvnError::InsufficientValidators {
            found: 0,
            required: config.validators_per_request,
        });
    }

    let required = config.validators_per_request;
    let mut selected = Vec::with_capacity(required);
    let mut used_operators = HashSet::new();
    let mut used_subnets = HashSet::new();

    // First Pass: Enforce strict per-operator and per-subnet diversity (max 1 seat per operator/subnet)
    while selected.len() < required {
        let candidate_indices: Vec<usize> = eligible
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let op_conflict = c
                    .operator_id
                    .as_deref()
                    .is_some_and(|op| used_operators.contains(op));
                let subnet_conflict = c
                    .ip_address
                    .as_deref()
                    .and_then(extract_ip_subnet)
                    .is_some_and(|sub| used_subnets.contains(&sub));
                !op_conflict && !subnet_conflict
            })
            .map(|(idx, _)| idx)
            .collect();

        if candidate_indices.is_empty() {
            // No more diverse candidates remaining in pool
            break;
        }

        let weights: Vec<f64> = candidate_indices
            .iter()
            .map(|&idx| (eligible[idx].karma as f64).powf(config.karma_pow))
            .collect();

        let dist = match WeightedIndex::new(&weights) {
            Ok(d) => d,
            Err(_) => break,
        };

        let chosen_idx_in_candidates = dist.sample(rng);
        let chosen_eligible_idx = candidate_indices[chosen_idx_in_candidates];
        let chosen = eligible.remove(chosen_eligible_idx);

        if let Some(op) = &chosen.operator_id {
            used_operators.insert(op.clone());
        }
        if let Some(sub) = chosen.ip_address.as_deref().and_then(extract_ip_subnet) {
            used_subnets.insert(sub);
        }

        selected.push(chosen);
    }

    // Second Pass: Graceful Fallback if pool has < 5 distinct operators/subnets
    while selected.len() < required && !eligible.is_empty() {
        let weights: Vec<f64> = eligible
            .iter()
            .map(|c| (c.karma as f64).powf(config.karma_pow))
            .collect();

        let dist = match WeightedIndex::new(&weights) {
            Ok(d) => d,
            Err(_) => break,
        };

        let chosen_idx = dist.sample(rng);
        let chosen = eligible.remove(chosen_idx);
        selected.push(chosen);
    }

    if selected.len() < required {
        return Err(IvnError::InsufficientValidators {
            found: selected.len(),
            required,
        });
    }

    Ok(selected)
}

// ---------------------------------------------------------------------------
// HTTP Handlers
// ---------------------------------------------------------------------------

/// `POST /v1/identity/request` — Create an identity verification request.
///
/// Selects 5 karma-weighted validators from the node pool, sets status to `Pending`,
/// and records the initial request.
pub async fn create_identity_request_handler(
    claims: Option<Extension<Claims>>,
    Json(payload): Json<CreateIdentityRequest>,
) -> impl IntoResponse {
    if !claims
        .as_ref()
        .is_some_and(|Extension(c)| c.role.can_add_memory())
    {
        return error_response(StatusCode::FORBIDDEN, "Write permission required").into_response();
    }
    let now = current_timestamp();
    let request_id = format!("ivn_req_{}", ulid::Ulid::new());

    let pool = if let Some(candidates) = payload.candidate_pool {
        candidates
    } else {
        default_validator_pool()
    };

    let submitter_id = payload
        .submitter_node_id
        .as_deref()
        .unwrap_or(payload.applicant.as_str());

    let exclude_seed = payload
        .seed
        .as_deref()
        .unwrap_or(payload.applicant.as_str());

    // VRF-salted pseudo-random entropy derived from task_id, timestamp, applicant, submitter, seed, and signature
    let mut hasher = Sha256::new();
    hasher.update(request_id.as_bytes());
    hasher.update(now.to_be_bytes());
    hasher.update(payload.applicant.as_bytes());
    hasher.update(submitter_id.as_bytes());
    hasher.update(exclude_seed.as_bytes());
    if let Some(sig) = &payload.signature {
        hasher.update(sig.as_bytes());
    }
    let vrf_seed: [u8; 32] = hasher.finalize().into();
    let seed_u64 = u64::from_le_bytes(vrf_seed[0..8].try_into().unwrap());
    let mut rng = StdRng::seed_from_u64(seed_u64);

    let selected_validators = match select_validators_with_diversity(
        &pool,
        submitter_id,
        &payload.applicant,
        exclude_seed,
        &mut rng,
    ) {
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

    let record = IdentityRequestRecord {
        id: request_id.clone(),
        applicant: payload.applicant,
        proof_hashes: payload.proof_hashes,
        signature: payload.signature,
        assigned_validators: selected_validators,
        votes: HashMap::new(),
        status: IdentityRequestStatus::Pending,
        created_at: now,
        updated_at: now,
        verdict: None,
    };

    {
        let mut store = current_ivn_store_mut();
        let mut updated = store.requests.clone();
        updated.insert(request_id.clone(), record.clone());
        if let Err(err) = store.save_requests(&updated) {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
        }
        store.requests = updated;
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    crate::isolate_test_process!();
    use super::*;

    #[test]
    fn test_extract_ip_subnet_ipv4_and_ipv6() {
        assert_eq!(
            extract_ip_subnet("192.168.1.50"),
            Some("192.168.1.0/24".into())
        );
        assert_eq!(
            extract_ip_subnet("192.168.1.200"),
            Some("192.168.1.0/24".into())
        );
        assert_eq!(extract_ip_subnet("10.0.1.1"), Some("10.0.1.0/24".into()));
        assert_eq!(
            extract_ip_subnet("2001:db8:abcd:0012::1"),
            Some("2001:db8:abcd:12::/64".into())
        );
        assert_eq!(extract_ip_subnet("invalid_ip"), None);
    }

    #[test]
    fn test_self_exclusion_rule() {
        let submitter = "node_submitter_x";
        let mut rng = StdRng::seed_from_u64(12345);

        let mut pool = vec![ValidatorCandidateDto {
            node_id: submitter.to_string(),
            wallet: "wallet_submitter".into(),
            karma: 1000,
            seed: "seed_submitter".into(),
            operator_id: Some("op_submitter".into()),
            ip_address: Some("192.168.0.1".into()),
        }];

        for i in 1..=6 {
            pool.push(ValidatorCandidateDto {
                node_id: format!("node_{}", i),
                wallet: format!("wallet_{}", i),
                karma: 500 + i * 50,
                seed: format!("seed_{}", i),
                operator_id: Some(format!("op_{}", i)),
                ip_address: Some(format!("192.168.{}.1", i)),
            });
        }

        let selected = select_validators_with_diversity(
            &pool,
            submitter,
            submitter,
            "seed_submitter",
            &mut rng,
        )
        .expect("Validator selection should succeed");

        assert_eq!(selected.len(), 5);
        for val in &selected {
            assert_ne!(val.node_id, submitter);
            assert_ne!(val.wallet, "wallet_submitter");
            assert_ne!(val.seed, "seed_submitter");
        }
    }

    #[test]
    fn test_per_operator_and_subnet_cap() {
        let mut rng = StdRng::seed_from_u64(9999);

        // Construct pool with clusters sharing operator_id and IP subnets
        let pool = vec![
            // Operator A cluster (3 nodes)
            ValidatorCandidateDto {
                node_id: "node_op_a_1".into(),
                wallet: "w_a1".into(),
                karma: 800,
                seed: "s_a1".into(),
                operator_id: Some("op_A".into()),
                ip_address: Some("10.0.1.10".into()), // subnet 10.0.1.0/24
            },
            ValidatorCandidateDto {
                node_id: "node_op_a_2".into(),
                wallet: "w_a2".into(),
                karma: 850,
                seed: "s_a2".into(),
                operator_id: Some("op_A".into()),
                ip_address: Some("10.0.1.20".into()), // subnet 10.0.1.0/24
            },
            ValidatorCandidateDto {
                node_id: "node_op_a_3".into(),
                wallet: "w_a3".into(),
                karma: 900,
                seed: "s_a3".into(),
                operator_id: Some("op_A".into()),
                ip_address: Some("10.0.1.30".into()), // subnet 10.0.1.0/24
            },
            // Operator B cluster (2 nodes in same subnet)
            ValidatorCandidateDto {
                node_id: "node_op_b_1".into(),
                wallet: "w_b1".into(),
                karma: 700,
                seed: "s_b1".into(),
                operator_id: Some("op_B".into()),
                ip_address: Some("10.0.2.10".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_op_b_2".into(),
                wallet: "w_b2".into(),
                karma: 750,
                seed: "s_b2".into(),
                operator_id: Some("op_B".into()),
                ip_address: Some("10.0.2.20".into()),
            },
            // Operators C, D, E (1 node each)
            ValidatorCandidateDto {
                node_id: "node_op_c".into(),
                wallet: "w_c".into(),
                karma: 600,
                seed: "s_c".into(),
                operator_id: Some("op_C".into()),
                ip_address: Some("10.0.3.10".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_op_d".into(),
                wallet: "w_d".into(),
                karma: 650,
                seed: "s_d".into(),
                operator_id: Some("op_D".into()),
                ip_address: Some("10.0.4.10".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_op_e".into(),
                wallet: "w_e".into(),
                karma: 950,
                seed: "s_e".into(),
                operator_id: Some("op_E".into()),
                ip_address: Some("10.0.5.10".into()),
            },
        ];

        let selected = select_validators_with_diversity(
            &pool,
            "applicant_submitter",
            "applicant_submitter",
            "applicant_seed",
            &mut rng,
        )
        .expect("Selection should succeed");

        assert_eq!(selected.len(), 5);

        let mut seen_ops = HashSet::new();
        let mut seen_subnets = HashSet::new();

        for val in &selected {
            if let Some(op) = &val.operator_id {
                assert!(
                    seen_ops.insert(op.clone()),
                    "Duplicate operator_id in 5-committee: {}",
                    op
                );
            }
            if let Some(sub) = val.ip_address.as_deref().and_then(extract_ip_subnet) {
                assert!(
                    seen_subnets.insert(sub.clone()),
                    "Duplicate subnet in 5-committee: {}",
                    sub
                );
            }
        }
    }

    #[test]
    fn test_vrf_entropy_selection_determinism() {
        let pool = default_validator_pool();
        let req1 = CreateIdentityRequest {
            applicant: "app_1".into(),
            proof_hashes: vec!["hash_1".into()],
            signature: Some("sig_abc".into()),
            candidate_pool: Some(pool.clone()),
            seed: Some("seed_123".into()),
            submitter_node_id: Some("submitter_node_1".into()),
        };

        // Same inputs yield same seed
        let mut hasher1 = Sha256::new();
        hasher1.update(b"test_req_id");
        hasher1.update(1000u64.to_be_bytes());
        hasher1.update(req1.applicant.as_bytes());
        hasher1.update(req1.submitter_node_id.as_ref().unwrap().as_bytes());
        hasher1.update(req1.seed.as_ref().unwrap().as_bytes());
        if let Some(sig) = &req1.signature {
            hasher1.update(sig.as_bytes());
        }
        let vrf_seed1: [u8; 32] = hasher1.finalize().into();
        let u64_seed1 = u64::from_le_bytes(vrf_seed1[0..8].try_into().unwrap());

        let mut hasher2 = Sha256::new();
        hasher2.update(b"test_req_id");
        hasher2.update(1000u64.to_be_bytes());
        hasher2.update(req1.applicant.as_bytes());
        hasher2.update(req1.submitter_node_id.as_ref().unwrap().as_bytes());
        hasher2.update(req1.seed.as_ref().unwrap().as_bytes());
        if let Some(sig) = &req1.signature {
            hasher2.update(sig.as_bytes());
        }
        let vrf_seed2: [u8; 32] = hasher2.finalize().into();
        let u64_seed2 = u64::from_le_bytes(vrf_seed2[0..8].try_into().unwrap());

        assert_eq!(u64_seed1, u64_seed2);

        let mut rng1 = StdRng::seed_from_u64(u64_seed1);
        let mut rng2 = StdRng::seed_from_u64(u64_seed2);

        let sel1 = select_validators_with_diversity(
            &pool,
            "submitter_node_1",
            "app_1",
            "seed_123",
            &mut rng1,
        )
        .unwrap();
        let sel2 = select_validators_with_diversity(
            &pool,
            "submitter_node_1",
            "app_1",
            "seed_123",
            &mut rng2,
        )
        .unwrap();

        assert_eq!(sel1.len(), 5);
        assert_eq!(
            sel1.iter().map(|v| &v.node_id).collect::<Vec<_>>(),
            sel2.iter().map(|v| &v.node_id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_graceful_fallback_when_few_operators() {
        let mut rng = StdRng::seed_from_u64(42);

        // Pool has 6 candidates total, but only 2 operators
        let pool = vec![
            ValidatorCandidateDto {
                node_id: "node_1".into(),
                wallet: "w_1".into(),
                karma: 500,
                seed: "s_1".into(),
                operator_id: Some("op_1".into()),
                ip_address: Some("192.168.1.1".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_2".into(),
                wallet: "w_2".into(),
                karma: 600,
                seed: "s_2".into(),
                operator_id: Some("op_1".into()),
                ip_address: Some("192.168.1.2".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_3".into(),
                wallet: "w_3".into(),
                karma: 700,
                seed: "s_3".into(),
                operator_id: Some("op_1".into()),
                ip_address: Some("192.168.1.3".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_4".into(),
                wallet: "w_4".into(),
                karma: 800,
                seed: "s_4".into(),
                operator_id: Some("op_2".into()),
                ip_address: Some("192.168.2.1".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_5".into(),
                wallet: "w_5".into(),
                karma: 900,
                seed: "s_5".into(),
                operator_id: Some("op_2".into()),
                ip_address: Some("192.168.2.2".into()),
            },
            ValidatorCandidateDto {
                node_id: "node_6".into(),
                wallet: "w_6".into(),
                karma: 1000,
                seed: "s_6".into(),
                operator_id: Some("op_2".into()),
                ip_address: Some("192.168.2.3".into()),
            },
        ];

        let selected = select_validators_with_diversity(
            &pool,
            "submitter_x",
            "applicant_x",
            "seed_x",
            &mut rng,
        )
        .expect("Selection should gracefully fall back and return 5 validators");

        assert_eq!(selected.len(), 5);
        for val in &selected {
            assert_ne!(val.node_id, "submitter_x");
        }
    }

    #[tokio::test]
    async fn test_final_verdict_applies_sanctions_and_exclusions() {
        use crate::data_commons::ivn::{clear_exclusions, is_excluded_at};
        use axum::extract::Path;
        use axum::Json;

        let tmp_dir = tempfile::tempdir().expect("tempdir");
        struct RestoreCwd(std::path::PathBuf);
        impl Drop for RestoreCwd {
            fn drop(&mut self) {
                std::env::set_current_dir(&self.0).unwrap();
            }
        }
        let _cwd = RestoreCwd(std::env::current_dir().unwrap());
        std::env::set_current_dir(tmp_dir.path()).unwrap();
        let prev_data_dir = std::env::var_os("XAVIER_DATA_DIR");
        std::env::set_var("XAVIER_DATA_DIR", tmp_dir.path());
        clear_exclusions();

        let req_id = "test_req_sanctions_1".to_string();
        let applicant = "xv1_applicant_liar".to_string();
        let val1 = "xv1_val_honest_1".to_string();
        let val2 = "xv1_val_honest_2".to_string();
        let val3 = "xv1_val_honest_3".to_string();
        let val4 = "xv1_val_honest_4".to_string();
        let val5_fp = "xv1_val_false_positive".to_string();

        let mut store = IvnEngineStore::default();
        store.requests.insert(
            req_id.clone(),
            IdentityRequestRecord {
                id: req_id.clone(),
                applicant: applicant.clone(),
                proof_hashes: vec!["hash".into()],
                signature: None,
                assigned_validators: vec![
                    ValidatorCandidateDto {
                        node_id: val1.clone(),
                        wallet: val1.clone(),
                        karma: 500,
                        seed: "s1".into(),
                        operator_id: None,
                        ip_address: None,
                    },
                    ValidatorCandidateDto {
                        node_id: val2.clone(),
                        wallet: val2.clone(),
                        karma: 500,
                        seed: "s2".into(),
                        operator_id: None,
                        ip_address: None,
                    },
                    ValidatorCandidateDto {
                        node_id: val3.clone(),
                        wallet: val3.clone(),
                        karma: 500,
                        seed: "s3".into(),
                        operator_id: None,
                        ip_address: None,
                    },
                    ValidatorCandidateDto {
                        node_id: val4.clone(),
                        wallet: val4.clone(),
                        karma: 500,
                        seed: "s4".into(),
                        operator_id: None,
                        ip_address: None,
                    },
                    ValidatorCandidateDto {
                        node_id: val5_fp.clone(),
                        wallet: val5_fp.clone(),
                        karma: 500,
                        seed: "s5".into(),
                        operator_id: None,
                        ip_address: None,
                    },
                ],
                votes: HashMap::new(),
                status: IdentityRequestStatus::Pending,
                created_at: 1_700_000_000,
                updated_at: 1_700_000_000,
                verdict: None,
            },
        );
        init_ivn_engine(store);

        // 4 honest validators vote Reject, 1 false-positive validator votes Check
        // Total 5 votes -> Quorum met -> Final status: Rejected!
        for v_id in [&val1, &val2, &val3, &val4] {
            let res = vote_identity_request_handler(
                Some(Extension(Claims::new(
                    v_id.to_string(),
                    String::new(),
                    crate::security::auth::UserRole::User,
                    chrono::Duration::hours(1),
                ))),
                Path(req_id.clone()),
                Json(VoteRequest {
                    validator_node_id: v_id.to_string(),
                    vote: Vote::Reject,
                }),
            )
            .await
            .into_response();
            assert_eq!(res.status(), StatusCode::OK);
        }

        let res5 = vote_identity_request_handler(
            Some(Extension(Claims::new(
                val5_fp.clone(),
                String::new(),
                crate::security::auth::UserRole::User,
                chrono::Duration::hours(1),
            ))),
            Path(req_id.clone()),
            Json(VoteRequest {
                validator_node_id: val5_fp.clone(),
                vote: Vote::Check,
            }),
        )
        .await
        .into_response();
        assert_eq!(res5.status(), StatusCode::OK);

        let final_record = current_ivn_store().requests.get(&req_id).unwrap().clone();
        assert_eq!(final_record.status, IdentityRequestStatus::Rejected);

        // Assert applicant is sanctioned (180 days retry window exclusion)
        let check_time = 1_700_000_000 + 86_400; // 1 day later
        assert!(is_excluded_at(
            &WalletAddress(applicant.clone()),
            check_time
        ));

        // Assert false-positive validator is sanctioned (90 days exclusion)
        assert!(is_excluded_at(&WalletAddress(val5_fp.clone()), check_time));

        // Assert honest reject validators are NOT excluded
        assert!(!is_excluded_at(&WalletAddress(val1.clone()), check_time));

        clear_exclusions();
        if let Some(prev) = prev_data_dir {
            std::env::set_var("XAVIER_DATA_DIR", prev);
        } else {
            std::env::remove_var("XAVIER_DATA_DIR");
        }
    }
}

/// `GET /v1/ivn/karma/{agent}` — Query agent karma, tier, and history log.
pub async fn get_ivn_karma_handler(Path(agent): Path<String>) -> impl IntoResponse {
    let store = current_ivn_store();
    let karma = store.karma_engine.get_karma(&agent);
    let tier = store.karma_engine.get_tier(&agent);
    let history = store
        .karma_engine
        .get_record(&agent)
        .map(|r| r.history.clone())
        .unwrap_or_default();

    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "agent": agent,
            "karma": karma,
            "tier": tier,
            "history": history
        })),
    )
        .into_response()
}

/// `GET /v1/identity/request/{id}` — Query status and votes for an identity request.
pub async fn get_identity_request_handler(Path(id): Path<String>) -> impl IntoResponse {
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
    claims: Option<Extension<Claims>>,
    Path(id): Path<String>,
    Json(payload): Json<VoteRequest>,
) -> impl IntoResponse {
    let Some(Extension(claims)) = claims else {
        return error_response(StatusCode::FORBIDDEN, "Missing authentication claims")
            .into_response();
    };
    if !claims.role.can_add_memory() {
        return error_response(StatusCode::FORBIDDEN, "Write permission required").into_response();
    }
    let now = current_timestamp();
    let mut store = current_ivn_store_mut();
    let mut updated = store.requests.clone();

    let record = match updated.get_mut(&id) {
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
    let is_authorized = record.assigned_validators.iter().any(|v| {
        (v.node_id == payload.validator_node_id || v.wallet == payload.validator_node_id)
            && (v.node_id == claims.sub || v.wallet == claims.sub)
    });

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

    if record.votes.contains_key(&claims.sub)
        || record
            .assigned_validators
            .iter()
            .filter(|v| v.node_id == claims.sub || v.wallet == claims.sub)
            .any(|v| record.votes.contains_key(&v.node_id) || record.votes.contains_key(&v.wallet))
    {
        return error_response(StatusCode::FORBIDDEN, "Principal has already voted")
            .into_response();
    }

    // Record the authenticated principal, never a caller-controlled voter key.
    record.votes.insert(claims.sub.clone(), payload.vote);
    record.updated_at = now;

    // Evaluate current votes using VerdictEngine
    let vote_vec: Vec<Vote> = record.votes.values().copied().collect();
    let config = IvnConfig::default();
    let verdict = VerdictEngine::evaluate_votes(&vote_vec, config.quorum_ratio);

    record.verdict = Some(VerdictDto::from(&verdict));

    let previous_status = record.status;
    match verdict.status {
        VerdictStatus::Passed => record.status = IdentityRequestStatus::Passed,
        VerdictStatus::Rejected if record.votes.len() >= record.assigned_validators.len() => {
            record.status = IdentityRequestStatus::Rejected;
        }
        VerdictStatus::QuorumNotMet if record.votes.len() >= record.assigned_validators.len() => {
            record.status = IdentityRequestStatus::QuorumNotMet;
        }
        _ => {}
    }
    let finalized = record.clone();

    let response = (
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
        .into_response();
    if let Err(err) = store.save_requests(&updated) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
    }
    store.requests = updated;
    // Persist the accepted vote before publishing verdict side effects.
    if finalized.status != previous_status
        && matches!(
            finalized.status,
            IdentityRequestStatus::Passed | IdentityRequestStatus::Rejected
        )
    {
        let mut engine = EigenTrustEngine::new(ReputationConfig::default(), Vec::new());
        let applicant = WalletAddress(finalized.applicant);
        let votes: Vec<(WalletAddress, Vote)> = finalized
            .votes
            .iter()
            .map(|(id, vote)| (WalletAddress(id.clone()), *vote))
            .collect();
        apply_rewards(&mut engine, &verdict, &applicant, &votes);
        let incorrect = if finalized.status == IdentityRequestStatus::Passed {
            Vote::Reject
        } else {
            Vote::Check
        };
        let validators: Vec<WalletAddress> = votes
            .iter()
            .filter(|(_, vote)| *vote == incorrect)
            .map(|(id, _)| id.clone())
            .collect();
        let liar = (finalized.status == IdentityRequestStatus::Rejected).then_some(&applicant);
        if !validators.is_empty() || liar.is_some() {
            apply_sanctions(&mut engine, &validators, liar, now);
        }
    }
    response
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
