//! Maloca domain types — mirrored by `@swal/maloca-client`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupportTicket {
    pub id: String,
    pub title: String,
    pub body: String,
    pub status: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub id: String,
    pub target: String,
    pub kind: String,
    pub notes: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MicroTask {
    pub id: String,
    pub parent_feature: String,
    pub kind: String,
    pub title: String,
    pub acceptance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_paths: Option<Vec<String>>,
    pub reward_hint: f64,
    pub difficulty: i32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshTicketOffer {
    pub id: String,
    pub microtask: MicroTask,
    pub offered_at: String,
    pub expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardReceipt {
    pub ticket_id: String,
    pub xp: i64,
    pub karma_delta: i64,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MalocaPack {
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codegraph_indexed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codegraph_head: Option<String>,
    pub features_total: u64,
    pub features_draft: u64,
    pub gaps_zero_symbol_modules: Vec<String>,
    pub decisions_count: u64,
    pub support_open: u64,
    pub inbox_open: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Open,
    Closed,
    Reconsidering,
    Analyzing,
}

impl ProposalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Reconsidering => "reconsidering",
            Self::Analyzing => "analyzing",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    #[serde(rename = "type")]
    pub proposal_type: String,
    pub title: String,
    pub body: String,
    pub status: ProposalStatus,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked_param: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagerActionType {
    RequestReconsideration,
    RequestScenarioAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerAction {
    pub id: String,
    #[serde(rename = "type")]
    pub action_type: ManagerActionType,
    #[serde(rename = "proposalId")]
    pub proposal_id: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkParam {
    pub key: String,
    pub default: String,
    pub locked_until_quorum: bool,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSupportBody {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub feature_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimBody {
    #[serde(default = "default_node_id")]
    pub node_id: String,
}

fn default_node_id() -> String {
    "local".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProposalBody {
    #[serde(rename = "type")]
    pub proposal_type: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub locked_param: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerActionBody {
    #[serde(rename = "type")]
    pub action_type: ManagerActionType,
    #[serde(rename = "proposalId")]
    pub proposal_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSnapshot {
    /// `"mock"` until edge-mesh bridge (P2P real = fuera de ola).
    pub mode: String,
    pub genesis_node_id: String,
    pub parent_nodes_enabled: bool,
    pub manager_adds_vote_weight: bool,
    pub wallet_multi_node_anchor: bool,
    pub nodes: Vec<MeshNodeInfo>,
    pub meshes: Vec<MeshInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshNodeInfo {
    pub node_id: String,
    pub role: String,
    pub note: String,
    pub karma: i64,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VoteChoice {
    Yes,
    No,
    Abstain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub id: String,
    pub proposal_id: String,
    pub node_id: String,
    pub choice: VoteChoice,
    /// Stub weight for this slice (1.0); full 50/25/25 formula later.
    pub weight: f64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastVoteBody {
    #[serde(default = "default_node_id")]
    pub node_id: String,
    pub choice: VoteChoice,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    ProposalCreated,
    Voted,
    ManagerRequestReconsideration,
    ManagerRequestScenarioAnalysis,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEvent {
    pub id: String,
    pub kind: DecisionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub actor_node_id: String,
    /// Always `lab_genesis` — history anchored to genesis.
    pub genesis_node_id: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node_id: String,
    pub role: String,
    pub karma: i64,
    pub active: bool,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshInfo {
    pub id: String,
    pub kind: String,
    pub description: String,
}
