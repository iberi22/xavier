//! In-memory Maloca store with JSON file persistence.
//!
//! Manager actions never add vote weight — they only flip proposal status
//! and append an audit trail (see NODE_MESH_MANAGER.md).
//! Votes require karma >= vote_karma_min + active node; history anchors to lab_genesis.

use super::params::network_parameters;
use super::types::*;
use anyhow::{bail, Result};
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const GENESIS_NODE_ID: &str = "lab_genesis";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    support: Vec<SupportTicket>,
    reviews: Vec<ReviewRequest>,
    inbox: Vec<MeshTicketOffer>,
    rewards: Vec<RewardReceipt>,
    proposals: Vec<Proposal>,
    manager_actions: Vec<ManagerAction>,
    #[serde(default)]
    votes: Vec<Vote>,
    #[serde(default)]
    decisions: Vec<DecisionEvent>,
    #[serde(default)]
    nodes: Vec<NodeRecord>,
    backlog: serde_json::Value,
}

pub struct MalocaStore {
    inner: RwLock<PersistedState>,
    path: PathBuf,
}

impl MalocaStore {
    pub fn open(state_dir: &Path) -> Arc<Self> {
        let path = state_dir.join("maloca").join("store.json");
        let mut state = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_else(default_state)
        } else {
            default_state()
        };
        if state.nodes.is_empty() {
            state.nodes = default_nodes();
        }
        if state.decisions.is_empty() && !state.proposals.is_empty() {
            // Bootstrap history for seed / migrated stores.
            for p in &state.proposals {
                state.decisions.push(DecisionEvent {
                    id: format!("d-{}", short_id()),
                    kind: DecisionKind::ProposalCreated,
                    proposal_id: Some(p.id.clone()),
                    actor_node_id: GENESIS_NODE_ID.into(),
                    genesis_node_id: GENESIS_NODE_ID.into(),
                    payload: serde_json::json!({ "title": p.title, "type": p.proposal_type }),
                    created_at: p.created_at.clone(),
                });
            }
        }
        Arc::new(Self {
            inner: RwLock::new(state),
            path,
        })
    }

    fn persist(&self, state: &PersistedState) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(raw) = serde_json::to_string_pretty(state) {
            let _ = std::fs::write(&self.path, raw);
        }
    }

    fn vote_karma_min(state: &PersistedState) -> i64 {
        let _ = state;
        network_parameters()
            .into_iter()
            .find(|p| p.key == "vote_karma_min")
            .and_then(|p| p.default.parse().ok())
            .unwrap_or(500)
    }

    fn append_decision(
        state: &mut PersistedState,
        kind: DecisionKind,
        proposal_id: Option<String>,
        actor_node_id: &str,
        payload: serde_json::Value,
    ) {
        state.decisions.insert(
            0,
            DecisionEvent {
                id: format!("d-{}", short_id()),
                kind,
                proposal_id,
                actor_node_id: actor_node_id.into(),
                genesis_node_id: GENESIS_NODE_ID.into(),
                payload,
                created_at: Utc::now().to_rfc3339(),
            },
        );
    }

    pub fn pack(&self) -> MalocaPack {
        let g = self.inner.read();
        MalocaPack {
            generated_at: Utc::now().to_rfc3339(),
            codegraph_indexed_at: None,
            codegraph_head: None,
            features_total: 0,
            features_draft: 0,
            gaps_zero_symbol_modules: vec![],
            decisions_count: g.decisions.len() as u64,
            support_open: g
                .support
                .iter()
                .filter(|t| t.status == "open")
                .count() as u64,
            inbox_open: g
                .inbox
                .iter()
                .filter(|o| o.claimed_by.is_none())
                .count() as u64,
        }
    }

    pub fn backlog(&self) -> serde_json::Value {
        self.inner.read().backlog.clone()
    }

    pub fn list_support(&self) -> Vec<SupportTicket> {
        self.inner.read().support.clone()
    }

    pub fn create_support(&self, body: CreateSupportBody) -> SupportTicket {
        let ticket = SupportTicket {
            id: format!("s-{}", short_id()),
            title: body.title,
            body: body.body,
            status: "open".into(),
            created_at: Utc::now().to_rfc3339(),
            feature_id: body.feature_id,
        };
        let mut g = self.inner.write();
        g.support.insert(0, ticket.clone());
        self.persist(&g);
        ticket
    }

    pub fn list_reviews(&self) -> Vec<ReviewRequest> {
        self.inner.read().reviews.clone()
    }

    pub fn list_inbox(&self) -> Vec<MeshTicketOffer> {
        self.inner.read().inbox.clone()
    }

    pub fn claim(&self, id: &str, node_id: &str) -> Result<MeshTicketOffer> {
        let mut g = self.inner.write();
        let offer = g
            .inbox
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| anyhow::anyhow!("inbox offer not found"))?;
        offer.claimed_by = Some(node_id.to_string());
        offer.microtask.status = "claimed".into();
        let out = offer.clone();
        self.persist(&g);
        Ok(out)
    }

    pub fn complete(&self, id: &str) -> Result<RewardReceipt> {
        let mut g = self.inner.write();
        let offer = g
            .inbox
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| anyhow::anyhow!("inbox offer not found"))?;
        offer.microtask.status = "completed".into();
        let claimed_by = offer.claimed_by.clone();
        let receipt = RewardReceipt {
            ticket_id: id.to_string(),
            xp: offer.microtask.reward_hint as i64,
            karma_delta: 1,
            recorded_at: Utc::now().to_rfc3339(),
        };
        if let Some(node_id) = claimed_by {
            if let Some(node) = g.nodes.iter_mut().find(|n| n.node_id == node_id) {
                node.karma = node.karma.saturating_add(receipt.karma_delta);
            }
        }
        g.rewards.insert(0, receipt.clone());
        self.persist(&g);
        Ok(receipt)
    }

    pub fn rewards(&self) -> Vec<RewardReceipt> {
        self.inner.read().rewards.clone()
    }

    pub fn list_nodes(&self) -> Vec<NodeRecord> {
        self.inner.read().nodes.clone()
    }

    pub fn mesh(&self) -> MeshSnapshot {
        let g = self.inner.read();
        MeshSnapshot {
            mode: "mock".into(),
            genesis_node_id: GENESIS_NODE_ID.into(),
            parent_nodes_enabled: false,
            manager_adds_vote_weight: false,
            wallet_multi_node_anchor: true,
            nodes: g
                .nodes
                .iter()
                .map(|n| MeshNodeInfo {
                    node_id: n.node_id.clone(),
                    role: n.role.clone(),
                    note: n.note.clone(),
                    karma: n.karma,
                    active: n.active,
                })
                .collect(),
            meshes: vec![
                MeshInfo {
                    id: "public_service_mesh".into(),
                    kind: "public".into(),
                    description: "Dividendos por evidencia de servicio".into(),
                },
                MeshInfo {
                    id: "swal/labs/brain".into(),
                    kind: "private_brain".into(),
                    description: "Cerebro privado; gerente sin peso de voto extra".into(),
                },
            ],
        }
    }

    pub fn params(&self) -> Vec<NetworkParam> {
        network_parameters()
    }

    pub fn list_proposals(&self) -> Vec<Proposal> {
        self.inner.read().proposals.clone()
    }

    pub fn create_proposal(&self, body: CreateProposalBody) -> Proposal {
        let locked = body.locked_param.or_else(|| {
            if body.proposal_type == "network_parameter" {
                Some(true)
            } else {
                None
            }
        });
        let proposal = Proposal {
            id: format!("p-{}", short_id()),
            proposal_type: body.proposal_type,
            title: body.title,
            body: body.body,
            status: ProposalStatus::Open,
            created_at: Utc::now().to_rfc3339(),
            locked_param: locked,
        };
        let mut g = self.inner.write();
        Self::append_decision(
            &mut g,
            DecisionKind::ProposalCreated,
            Some(proposal.id.clone()),
            GENESIS_NODE_ID,
            serde_json::json!({
                "title": proposal.title,
                "type": proposal.proposal_type,
            }),
        );
        g.proposals.insert(0, proposal.clone());
        self.persist(&g);
        proposal
    }

    pub fn list_votes(&self, proposal_id: Option<&str>) -> Vec<Vote> {
        let g = self.inner.read();
        match proposal_id {
            Some(pid) => g
                .votes
                .iter()
                .filter(|v| v.proposal_id == pid)
                .cloned()
                .collect(),
            None => g.votes.clone(),
        }
    }

    pub fn list_decisions(&self) -> Vec<DecisionEvent> {
        self.inner.read().decisions.clone()
    }

    /// Cast a vote. Requires active node with karma >= vote_karma_min.
    /// Manager role never adds extra weight (weight stub = 1.0).
    pub fn cast_vote(&self, proposal_id: &str, body: CastVoteBody) -> Result<Vote> {
        let mut g = self.inner.write();
        let min_karma = Self::vote_karma_min(&g);

        let node = g
            .nodes
            .iter()
            .find(|n| n.node_id == body.node_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("node not found"))?;

        if !node.active {
            bail!("node is not active");
        }
        if node.karma < min_karma {
            bail!(
                "karma {} below vote_karma_min {} (node {})",
                node.karma,
                min_karma,
                node.node_id
            );
        }

        let proposal = g
            .proposals
            .iter()
            .find(|p| p.id == proposal_id)
            .ok_or_else(|| anyhow::anyhow!("proposal not found"))?;

        if proposal.status == ProposalStatus::Closed {
            bail!("proposal is closed");
        }

        if g.votes
            .iter()
            .any(|v| v.proposal_id == proposal_id && v.node_id == body.node_id)
        {
            bail!("node already voted on this proposal");
        }

        let vote = Vote {
            id: format!("v-{}", short_id()),
            proposal_id: proposal_id.to_string(),
            node_id: body.node_id.clone(),
            choice: body.choice.clone(),
            weight: 1.0,
            created_at: Utc::now().to_rfc3339(),
        };

        Self::append_decision(
            &mut g,
            DecisionKind::Voted,
            Some(proposal_id.to_string()),
            &body.node_id,
            serde_json::json!({
                "choice": vote.choice,
                "weight": vote.weight,
                "karma": node.karma,
            }),
        );
        g.votes.insert(0, vote.clone());
        self.persist(&g);
        Ok(vote)
    }

    pub fn list_manager_actions(&self) -> Vec<ManagerAction> {
        self.inner.read().manager_actions.clone()
    }

    /// Manager may request reconsideration / scenario analysis.
    /// Never changes vote weight (`manager_adds_vote_weight` stays false).
    pub fn manager_action(&self, body: ManagerActionBody) -> Result<ManagerAction> {
        let mut g = self.inner.write();
        let proposal = g
            .proposals
            .iter_mut()
            .find(|p| p.id == body.proposal_id)
            .ok_or_else(|| anyhow::anyhow!("proposal not found"))?;

        if proposal.status != ProposalStatus::Open
            && proposal.status != ProposalStatus::Reconsidering
            && proposal.status != ProposalStatus::Analyzing
        {
            bail!("proposal is closed");
        }

        // Economic / locked params cannot be silently unlocked by manager UI.
        if proposal.locked_param == Some(true)
            && matches!(body.action_type, ManagerActionType::RequestReconsideration)
        {
            // Allowed: reconsideration still does not unlock — only status flip.
        }

        let kind = match body.action_type {
            ManagerActionType::RequestReconsideration => {
                proposal.status = ProposalStatus::Reconsidering;
                DecisionKind::ManagerRequestReconsideration
            }
            ManagerActionType::RequestScenarioAnalysis => {
                proposal.status = ProposalStatus::Analyzing;
                DecisionKind::ManagerRequestScenarioAnalysis
            }
        };

        let action = ManagerAction {
            id: format!("m-{}", short_id()),
            action_type: body.action_type,
            proposal_id: body.proposal_id.clone(),
            reason: body.reason.clone(),
            created_at: Utc::now().to_rfc3339(),
        };
        Self::append_decision(
            &mut g,
            kind,
            Some(body.proposal_id),
            GENESIS_NODE_ID,
            serde_json::json!({ "reason": body.reason, "adds_vote_weight": false }),
        );
        g.manager_actions.insert(0, action.clone());
        self.persist(&g);
        Ok(action)
    }
}

fn short_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

fn default_nodes() -> Vec<NodeRecord> {
    vec![
        NodeRecord {
            node_id: GENESIS_NODE_ID.into(),
            role: "genesis".into(),
            karma: 1000,
            active: true,
            note: "Fundador SWAL; gerente ACL inicial".into(),
        },
        NodeRecord {
            node_id: "local".into(),
            role: "peer".into(),
            karma: 50,
            active: true,
            note: "Nodo local dogfood (karma bajo — rechazo de voto)".into(),
        },
    ]
}

fn default_state() -> PersistedState {
    let now = Utc::now().to_rfc3339();
    let seed = Proposal {
        id: "p-genesis-params".into(),
        proposal_type: "network_parameter".into(),
        title: "Inscribir NetworkParameters (Lab)".into(),
        body: "Defaults lab_genesis + locked_until_quorum. Lectura pública.".into(),
        status: ProposalStatus::Open,
        created_at: now.clone(),
        locked_param: Some(true),
    };
    PersistedState {
        support: vec![],
        reviews: vec![],
        inbox: vec![MeshTicketOffer {
            id: "offer-seed-1".into(),
            microtask: MicroTask {
                id: "mt-seed-1".into(),
                parent_feature: "maloca-ops".into(),
                kind: "docs".into(),
                title: "Documentar dogfood Maloca en Xavier panel".into(),
                acceptance: "Sección docs + captura de tabs MalocaView".into(),
                evidence_paths: None,
                reward_hint: 10.0,
                difficulty: 1,
                status: "open".into(),
            },
            offered_at: now.clone(),
            expires_at: now.clone(),
            claimed_by: None,
        }],
        rewards: vec![],
        proposals: vec![seed.clone()],
        manager_actions: vec![],
        votes: vec![],
        decisions: vec![DecisionEvent {
            id: "d-genesis-seed".into(),
            kind: DecisionKind::ProposalCreated,
            proposal_id: Some(seed.id.clone()),
            actor_node_id: GENESIS_NODE_ID.into(),
            genesis_node_id: GENESIS_NODE_ID.into(),
            payload: serde_json::json!({
                "title": seed.title,
                "type": seed.proposal_type,
            }),
            created_at: now,
        }],
        nodes: default_nodes(),
        backlog: serde_json::json!({
            "source": "xavier/src/maloca",
            "items": [
                { "id": "maloca-ui", "title": "MalocaView dogfood en panel-ui", "status": "in_progress" },
                { "id": "maloca-pwa", "title": "PWA Maloca (después de Xavier)", "status": "later" }
            ]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maloca::types::{
        CastVoteBody, ManagerActionBody, ManagerActionType, VoteChoice,
    };

    fn temp_store() -> (PathBuf, Arc<MalocaStore>) {
        let dir = std::env::temp_dir().join(format!("maloca-test-{}", short_id()));
        let store = MalocaStore::open(&dir);
        (dir, store)
    }

    #[test]
    fn manager_action_does_not_claim_vote_weight() {
        let (dir, store) = temp_store();
        let mesh = store.mesh();
        assert!(!mesh.manager_adds_vote_weight);
        assert_eq!(mesh.mode, "mock");
        let params = store.params();
        let vote_weight = params
            .iter()
            .find(|p| p.key == "manager_adds_vote_weight")
            .expect("param");
        assert_eq!(vote_weight.default, "false");

        let action = store
            .manager_action(ManagerActionBody {
                action_type: ManagerActionType::RequestReconsideration,
                proposal_id: "p-genesis-params".into(),
                reason: "test".into(),
            })
            .unwrap();
        assert_eq!(action.proposal_id, "p-genesis-params");
        let p = store
            .list_proposals()
            .into_iter()
            .find(|p| p.id == "p-genesis-params")
            .unwrap();
        assert_eq!(p.status, ProposalStatus::Reconsidering);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn vote_rejects_low_karma_node() {
        let (dir, store) = temp_store();
        let err = store
            .cast_vote(
                "p-genesis-params",
                CastVoteBody {
                    node_id: "local".into(),
                    choice: VoteChoice::Yes,
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains("vote_karma_min"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn vote_accepts_lab_genesis_and_anchors_decision() {
        let (dir, store) = temp_store();
        let vote = store
            .cast_vote(
                "p-genesis-params",
                CastVoteBody {
                    node_id: GENESIS_NODE_ID.into(),
                    choice: VoteChoice::Yes,
                },
            )
            .unwrap();
        assert_eq!(vote.node_id, GENESIS_NODE_ID);
        assert_eq!(vote.weight, 1.0);

        let decisions = store.list_decisions();
        let voted = decisions
            .iter()
            .find(|d| d.kind == DecisionKind::Voted)
            .expect("voted event");
        assert_eq!(voted.genesis_node_id, GENESIS_NODE_ID);
        assert_eq!(voted.actor_node_id, GENESIS_NODE_ID);
        assert!(store.pack().decisions_count >= 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manager_decision_event_has_no_vote_weight() {
        let (dir, store) = temp_store();
        store
            .manager_action(ManagerActionBody {
                action_type: ManagerActionType::RequestScenarioAnalysis,
                proposal_id: "p-genesis-params".into(),
                reason: "sim".into(),
            })
            .unwrap();
        let ev = store
            .list_decisions()
            .into_iter()
            .find(|d| d.kind == DecisionKind::ManagerRequestScenarioAnalysis)
            .expect("manager event");
        assert_eq!(ev.genesis_node_id, GENESIS_NODE_ID);
        assert_eq!(ev.payload["adds_vote_weight"], false);
        let _ = std::fs::remove_dir_all(dir);
    }
}
