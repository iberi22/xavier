//! In-memory Maloca store with JSON file persistence.
//!
//! Manager actions never add vote weight — they only flip proposal status
//! and append an audit trail (see NODE_MESH_MANAGER.md).

use super::params::network_parameters;
use super::types::*;
use anyhow::{bail, Result};
use chrono::Utc;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedState {
    support: Vec<SupportTicket>,
    reviews: Vec<ReviewRequest>,
    inbox: Vec<MeshTicketOffer>,
    rewards: Vec<RewardReceipt>,
    proposals: Vec<Proposal>,
    manager_actions: Vec<ManagerAction>,
    backlog: serde_json::Value,
}

pub struct MalocaStore {
    inner: RwLock<PersistedState>,
    path: PathBuf,
}

impl MalocaStore {
    pub fn open(state_dir: &Path) -> Arc<Self> {
        let path = state_dir.join("maloca").join("store.json");
        let state = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_else(default_state)
        } else {
            default_state()
        };
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

    pub fn pack(&self) -> MalocaPack {
        let g = self.inner.read();
        MalocaPack {
            generated_at: Utc::now().to_rfc3339(),
            codegraph_indexed_at: None,
            codegraph_head: None,
            features_total: 0,
            features_draft: 0,
            gaps_zero_symbol_modules: vec![],
            decisions_count: g.proposals.len() as u64,
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
        let receipt = RewardReceipt {
            ticket_id: id.to_string(),
            xp: offer.microtask.reward_hint as i64,
            karma_delta: 1,
            recorded_at: Utc::now().to_rfc3339(),
        };
        g.rewards.insert(0, receipt.clone());
        self.persist(&g);
        Ok(receipt)
    }

    pub fn rewards(&self) -> Vec<RewardReceipt> {
        self.inner.read().rewards.clone()
    }

    pub fn mesh(&self) -> MeshSnapshot {
        MeshSnapshot {
            genesis_node_id: "lab_genesis".into(),
            parent_nodes_enabled: false,
            manager_adds_vote_weight: false,
            wallet_multi_node_anchor: true,
            nodes: vec![
                MeshNodeInfo {
                    node_id: "lab_genesis".into(),
                    role: "genesis".into(),
                    note: "Fundador SWAL; gerente ACL inicial".into(),
                },
                MeshNodeInfo {
                    node_id: "local".into(),
                    role: "peer".into(),
                    note: "Nodo local dogfood".into(),
                },
            ],
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
        g.proposals.insert(0, proposal.clone());
        self.persist(&g);
        proposal
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

        proposal.status = match body.action_type {
            ManagerActionType::RequestReconsideration => ProposalStatus::Reconsidering,
            ManagerActionType::RequestScenarioAnalysis => ProposalStatus::Analyzing,
        };

        let action = ManagerAction {
            id: format!("m-{}", short_id()),
            action_type: body.action_type,
            proposal_id: body.proposal_id,
            reason: body.reason,
            created_at: Utc::now().to_rfc3339(),
        };
        g.manager_actions.insert(0, action.clone());
        self.persist(&g);
        Ok(action)
    }
}

fn short_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

fn default_state() -> PersistedState {
    let now = Utc::now().to_rfc3339();
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
        proposals: vec![Proposal {
            id: "p-genesis-params".into(),
            proposal_type: "network_parameter".into(),
            title: "Inscribir NetworkParameters (Lab)".into(),
            body: "Defaults lab_genesis + locked_until_quorum. Lectura pública.".into(),
            status: ProposalStatus::Open,
            created_at: now,
            locked_param: Some(true),
        }],
        manager_actions: vec![],
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
    use crate::maloca::types::{ManagerActionBody, ManagerActionType};

    #[test]
    fn manager_action_does_not_claim_vote_weight() {
        let dir = std::env::temp_dir().join(format!("maloca-test-{}", short_id()));
        let store = MalocaStore::open(&dir);
        let mesh = store.mesh();
        assert!(!mesh.manager_adds_vote_weight);
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
}
