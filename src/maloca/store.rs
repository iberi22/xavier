//! In-memory Maloca store with JSON file persistence.
//!
//! Manager actions never add vote weight — they only flip proposal status
//! and append an audit trail (see NODE_MESH_MANAGER.md).
//! Votes require karma >= vote_karma_min + active node; history anchors to lab_genesis.

use super::governance;
use super::params::network_parameters;
use super::support_flow::{self, SupportStatus};
use super::types::*;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::io::Write;
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

impl std::fmt::Debug for MalocaStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MalocaStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl MalocaStore {
    /// Resolve the single canonical base directory for the Maloca store file.
    ///
    /// `open()` appends `maloca/store.json` to whatever directory is returned
    /// here, so this is the ONE place that decides where that file lives.
    /// Historically three different call sites each guessed a different base
    /// dir (`XAVIER_STATE_DIR`/`HOME`, a relative `"data/maloca"`, and
    /// `settings.memory.data_dir`), which meant writes and reads silently
    /// diverged and a fresh/empty store could shadow the real one with ~377
    /// tickets. Preference order:
    ///
    /// 1. `XAVIER_STATE_DIR` — explicit operator/test override.
    /// 2. `settings.memory.data_dir` — the same data root the rest of
    ///    Xavier's memory subsystem uses; this is where the real store.json
    ///    lives in production (`data/maloca/store.json`).
    /// 3. `"data"` — matches `MemorySettings::default()` when no config file
    ///    or env var is present.
    pub fn resolve_state_dir() -> PathBuf {
        crate::settings::paths::state_dir()
    }

    /// Open the store at the canonical resolved directory (see
    /// [`Self::resolve_state_dir`]). Prefer this over `open()` with a
    /// manually constructed path — it is the single source of truth for
    /// where the Maloca store file lives, and keeps every call site (CLI
    /// server bootstrap, HTTP routes, self-manage CLI commands) consistent.
    pub fn open_default() -> Arc<Self> {
        Self::open(&Self::resolve_state_dir())
    }

    pub fn open(state_dir: &Path) -> Arc<Self> {
        let path = state_dir.join("maloca").join("store.json");
        let mut state = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(raw) => match serde_json::from_str::<PersistedState>(&raw) {
                    Ok(parsed) => parsed,
                    Err(parse_err) => {
                        tracing::error!(
                            path = %path.display(),
                            error = %parse_err,
                            "MalocaStore: store.json failed to parse — quarantining the file \
                             instead of silently discarding it, so the next persist() cannot \
                             overwrite real data with an empty seed state"
                        );
                        quarantine_corrupt_file(&path);
                        default_state()
                    }
                },
                Err(read_err) => {
                    tracing::error!(
                        path = %path.display(),
                        error = %read_err,
                        "MalocaStore: store.json exists but could not be read — starting from \
                         an empty seed state without touching the file on disk"
                    );
                    default_state()
                }
            }
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

    fn persist(&self, state: &PersistedState) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(state)?;
        write_atomic(&self.path, raw.as_bytes())
    }

    /// Persists the state, logging a failure instead of propagating it. The
    /// in-memory state stays authoritative for the running process.
    fn persist_or_log(&self, state: &PersistedState) {
        if let Err(err) = self.persist(state) {
            tracing::error!(
                path = %self.path.display(),
                error = %err,
                "MalocaStore: failed to persist store.json; keeping in-memory state"
            );
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
            support_open: g.support.iter().filter(|t| t.status == "open").count() as u64,
            inbox_open: g.inbox.iter().filter(|o| o.claimed_by.is_none()).count() as u64,
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
        self.persist_or_log(&g);
        ticket
    }

    /// Moves a support ticket through the support state machine. An unknown id
    /// fails with a plain error; a move the state machine rejects fails with
    /// `InvalidTransition`. The HTTP layer maps the first to 404, the second to 409.
    pub fn update_support_status(&self, id: &str, to: SupportStatus) -> Result<SupportTicket> {
        let mut g = self.inner.write();
        let ticket = g
            .support
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow::anyhow!("support ticket not found"))?;
        let from = parse_support_status(&ticket.status).unwrap_or(SupportStatus::Open);
        let next = support_flow::transition(from, to)?;
        ticket.status = support_status_name(next).into();
        let updated = ticket.clone();
        self.persist_or_log(&g);
        Ok(updated)
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
        if let Some(ref claimed) = offer.claimed_by {
            if claimed != node_id {
                return Err(anyhow::anyhow!("offer already claimed by a different node"));
            }
        }
        offer.claimed_by = Some(node_id.to_string());
        offer.microtask.status = "claimed".into();
        let out = offer.clone();
        self.persist_or_log(&g);
        Ok(out)
    }

    pub fn complete(&self, id: &str) -> Result<RewardReceipt> {
        let mut g = self.inner.write();
        let offer = g
            .inbox
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| anyhow::anyhow!("inbox offer not found"))?;
        if offer.microtask.status == "completed" {
            return Err(anyhow::anyhow!("offer already completed"));
        }
        if offer.claimed_by.is_none() {
            return Err(anyhow::anyhow!("offer not claimed"));
        }
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
        self.persist_or_log(&g);
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
        self.close_due_proposals(Utc::now());
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
        let created_at = Utc::now();
        let expires_at = expiry_after(created_at, network_param("proposal_duration_days", 7));
        let proposal = Proposal {
            id: format!("p-{}", short_id()),
            proposal_type: body.proposal_type,
            title: body.title,
            body: body.body,
            status: ProposalStatus::Open,
            created_at: created_at.to_rfc3339(),
            locked_param: locked,
            expires_at: expires_at.map(|t| t.to_rfc3339()),
            outcome: None,
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
        self.persist_or_log(&g);
        proposal
    }

    /// Closes every open proposal that is due at `now`. Each one is tallied over
    /// its current votes; eligible weight is the number of active nodes with
    /// karma >= `vote_karma_min` (1.0 each). Closed proposals are skipped, so a
    /// second call returns nothing. Returns the proposals closed by this call.
    pub fn close_due_proposals(&self, now: DateTime<Utc>) -> Vec<Proposal> {
        let mut g = self.inner.write();
        let duration_days = network_param("proposal_duration_days", 7);
        let quorum_pct = network_param("quorum_pct_eligible", 30.0);
        let min_karma = Self::vote_karma_min(&g);
        let eligible = g
            .nodes
            .iter()
            .filter(|n| n.active && n.karma >= min_karma)
            .count() as f64;

        let due: Vec<usize> = g
            .proposals
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.status == ProposalStatus::Open && proposal_is_due(p, duration_days, now)
            })
            .map(|(idx, _)| idx)
            .collect();
        if due.is_empty() {
            return Vec::new();
        }

        let mut closed = Vec::with_capacity(due.len());
        for idx in due {
            let proposal_id = g.proposals[idx].id.clone();
            let votes: Vec<Vote> = g
                .votes
                .iter()
                .filter(|v| v.proposal_id == proposal_id)
                .cloned()
                .collect();
            let tally = governance::tally(&votes, eligible);
            let label = match governance::outcome(&tally, quorum_pct, 0.5) {
                governance::Outcome::Approved => "approved",
                governance::Outcome::Rejected => "rejected",
                governance::Outcome::NoQuorum => "no_quorum",
            };
            g.proposals[idx].status = ProposalStatus::Closed;
            g.proposals[idx].outcome = Some(label.into());
            Self::append_decision(
                &mut g,
                DecisionKind::Closed,
                Some(proposal_id),
                GENESIS_NODE_ID,
                serde_json::json!({
                    "outcome": label,
                    "yes": tally.yes,
                    "no": tally.no,
                    "abstain": tally.abstain,
                    "eligible": tally.eligible,
                    "participation_pct": tally.participation_pct,
                }),
            );
            closed.push(g.proposals[idx].clone());
        }
        self.persist_or_log(&g);
        closed
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
        self.close_due_proposals(Utc::now());
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
        self.persist_or_log(&g);
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
        self.persist_or_log(&g);
        Ok(action)
    }

    #[cfg(test)]
    fn set_node_active(&self, node_id: &str, active: bool) {
        let mut g = self.inner.write();
        if let Some(n) = g.nodes.iter_mut().find(|n| n.node_id == node_id) {
            n.active = active;
        }
        self.persist_or_log(&g);
    }
}

/// Name stored in `SupportTicket.status` and accepted by `PATCH /maloca/support/{id}`.
pub fn support_status_name(status: SupportStatus) -> &'static str {
    match status {
        SupportStatus::Open => "open",
        SupportStatus::Triaged => "triaged",
        SupportStatus::InProgress => "in_progress",
        SupportStatus::Resolved => "resolved",
        SupportStatus::Closed => "closed",
    }
}

/// Inverse of [`support_status_name`]. Unknown names give `None`.
pub fn parse_support_status(raw: &str) -> Option<SupportStatus> {
    match raw {
        "open" => Some(SupportStatus::Open),
        "triaged" => Some(SupportStatus::Triaged),
        "in_progress" => Some(SupportStatus::InProgress),
        "resolved" => Some(SupportStatus::Resolved),
        "closed" => Some(SupportStatus::Closed),
        _ => None,
    }
}

/// Reads a network parameter by key, falling back when it is missing or unparsable.
fn network_param<T: std::str::FromStr>(key: &str, fallback: T) -> T {
    network_parameters()
        .into_iter()
        .find(|p| p.key == key)
        .and_then(|p| p.default.parse().ok())
        .unwrap_or(fallback)
}

/// `created_at + days`, or `None` when the duration is out of range (never expires).
fn expiry_after(created_at: DateTime<Utc>, days: i64) -> Option<DateTime<Utc>> {
    Duration::try_days(days).and_then(|d| created_at.checked_add_signed(d))
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Whether a proposal is due at `now`. A stored `expires_at` wins; proposals
/// saved before that field existed expire at `created_at + duration_days`.
fn proposal_is_due(proposal: &Proposal, duration_days: i64, now: DateTime<Utc>) -> bool {
    if let Some(expires_at) = proposal.expires_at.as_deref().and_then(parse_timestamp) {
        return now >= expires_at;
    }
    parse_timestamp(&proposal.created_at)
        .is_some_and(|created| governance::is_expired(created, duration_days, now))
}

fn short_id() -> String {
    Uuid::new_v4().to_string()[..8].to_string()
}

/// Writes `bytes` to `path` atomically: the content goes to a sibling temp file
/// that is fsynced and then renamed over `path`, so a crash leaves either the
/// old or the new file, never a torn one. The parent directory is fsynced
/// afterwards where the platform supports it.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .context("store path has no file name")?
        .to_string_lossy()
        .into_owned();
    let tmp = parent.join(format!("{file_name}.{}.tmp", Uuid::new_v4()));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(err) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(err).with_context(|| format!("failed to write {}", path.display()));
    }
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Renames an unparsable `store.json` out of the way (`store.json.corrupt-<ts>`)
/// so the corrupt bytes are preserved on disk instead of being lost the next
/// time `persist()` writes a fresh (empty) state to the original path.
fn quarantine_corrupt_file(path: &Path) {
    let ts = Utc::now().format("%Y%m%dT%H%M%SZ");
    let quarantined = path.with_extension(format!("json.corrupt-{ts}"));
    match std::fs::rename(path, &quarantined) {
        Ok(()) => {
            tracing::error!(
                quarantined_to = %quarantined.display(),
                "MalocaStore: corrupt store.json quarantined; starting empty seed state"
            );
        }
        Err(rename_err) => {
            tracing::error!(
                path = %path.display(),
                error = %rename_err,
                "MalocaStore: failed to quarantine corrupt store.json — the corrupt file was \
                 left in place; investigate manually before the next persist() runs"
            );
        }
    }
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
        expires_at: None,
        outcome: None,
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
        CastVoteBody, CreateSupportBody, ManagerActionBody, ManagerActionType, VoteChoice,
    };

    fn temp_store() -> (PathBuf, Arc<MalocaStore>) {
        let dir = std::env::temp_dir().join(format!("maloca-test-{}", short_id()));
        let store = MalocaStore::open(&dir);
        (dir, store)
    }

    /// Loading a real, on-disk `store.json` fixture must surface its actual
    /// data (not silently fall back to the empty seed state) — this is the
    /// scenario the three unified call sites (`MalocaStore::open_default`)
    /// must all land on consistently.
    #[test]
    fn open_loads_a_real_store_json_fixture_from_disk() {
        let dir = std::env::temp_dir().join(format!("maloca-fixture-test-{}", short_id()));
        let maloca_dir = dir.join("maloca");
        std::fs::create_dir_all(&maloca_dir).unwrap();

        let fixture = r#"{
            "support": [
                {
                    "id": "s-fixture001",
                    "title": "Real ticket loaded from a fixture store.json",
                    "body": "This must survive MalocaStore::open, not be discarded",
                    "status": "open",
                    "created_at": "2026-09-01T00:00:00Z"
                }
            ],
            "reviews": [],
            "inbox": [],
            "rewards": [],
            "proposals": [],
            "manager_actions": [],
            "backlog": {"source": "fixture", "items": []}
        }"#;
        std::fs::write(maloca_dir.join("store.json"), fixture).unwrap();

        let store = MalocaStore::open(&dir);

        let support = store.list_support();
        assert_eq!(support.len(), 1, "fixture's support ticket must be loaded");
        assert_eq!(support[0].id, "s-fixture001");
        assert_eq!(
            support[0].title,
            "Real ticket loaded from a fixture store.json"
        );

        // Bootstrap logic (empty `nodes` -> defaults) still runs correctly on
        // top of real, non-empty data loaded from disk.
        let mesh = store.mesh();
        assert_eq!(mesh.mode, "mock");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A `store.json` that fails to parse must NOT be silently discarded —
    /// discarding it would let the next `persist()` overwrite real data with
    /// an empty seed state. `open()` must quarantine the corrupt file
    /// (`store.json.corrupt-<ts>`) instead, and any subsequent write must
    /// land at the original path without touching the quarantined bytes.
    #[test]
    fn corrupt_store_json_is_quarantined_not_silently_overwritten() {
        let dir = std::env::temp_dir().join(format!("maloca-corrupt-test-{}", short_id()));
        let maloca_dir = dir.join("maloca");
        std::fs::create_dir_all(&maloca_dir).unwrap();

        let corrupt_bytes = "{ this is not valid json at all, truncated mid-object ";
        let store_path = maloca_dir.join("store.json");
        std::fs::write(&store_path, corrupt_bytes).unwrap();

        // Opening must not panic, and must start from an empty seed state.
        let store = MalocaStore::open(&dir);
        assert!(
            store.list_support().is_empty(),
            "corrupt file must not be parsed into fake support tickets"
        );

        // The corrupt bytes must have been preserved under a quarantine name,
        // not deleted, and the original path must no longer hold them.
        let entries: Vec<_> = std::fs::read_dir(&maloca_dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        let quarantined_name = entries
            .iter()
            .find(|name| name.starts_with("store.json.corrupt-"))
            .cloned()
            .expect("corrupt store.json must be quarantined under store.json.corrupt-<ts>");
        let quarantined_content =
            std::fs::read_to_string(maloca_dir.join(&quarantined_name)).unwrap();
        assert_eq!(
            quarantined_content, corrupt_bytes,
            "quarantined file must preserve the original corrupt bytes verbatim"
        );
        assert!(
            !store_path.exists(),
            "original store.json path must be vacated by the rename, not copied"
        );

        // A subsequent write (persist()) must create a *fresh* store.json at
        // the original path without touching the quarantined corrupt file.
        store.create_support(CreateSupportBody {
            title: "post-quarantine ticket".into(),
            body: "written after recovering from corruption".into(),
            feature_id: None,
        });
        assert!(
            store_path.exists(),
            "persist() must write a fresh store.json after quarantine"
        );
        let quarantined_after =
            std::fs::read_to_string(maloca_dir.join(&quarantined_name)).unwrap();
        assert_eq!(
            quarantined_after, corrupt_bytes,
            "quarantined file must remain untouched by later persist() calls"
        );

        let _ = std::fs::remove_dir_all(dir);
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

    #[test]
    fn create_proposal_appends_decision_anchored_to_genesis() {
        let (dir, store) = temp_store();
        let before = store.pack().decisions_count;
        let p = store.create_proposal(CreateProposalBody {
            proposal_type: "feature_request".into(),
            title: "Ship vote UI".into(),
            body: "panel dogfood".into(),
            locked_param: None,
        });
        assert_eq!(store.pack().decisions_count, before + 1);
        let ev = store
            .list_decisions()
            .into_iter()
            .find(|d| d.proposal_id.as_deref() == Some(p.id.as_str()))
            .expect("proposal_created");
        assert_eq!(ev.kind, DecisionKind::ProposalCreated);
        assert_eq!(ev.genesis_node_id, GENESIS_NODE_ID);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_vote_from_same_node_rejected() {
        let (dir, store) = temp_store();
        store
            .cast_vote(
                "p-genesis-params",
                CastVoteBody {
                    node_id: GENESIS_NODE_ID.into(),
                    choice: VoteChoice::Yes,
                },
            )
            .unwrap();
        let err = store
            .cast_vote(
                "p-genesis-params",
                CastVoteBody {
                    node_id: GENESIS_NODE_ID.into(),
                    choice: VoteChoice::No,
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains("already voted"));
        assert_eq!(store.list_votes(Some("p-genesis-params")).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn inactive_node_cannot_vote() {
        let (dir, store) = temp_store();
        store.set_node_active(GENESIS_NODE_ID, false);
        let err = store
            .cast_vote(
                "p-genesis-params",
                CastVoteBody {
                    node_id: GENESIS_NODE_ID.into(),
                    choice: VoteChoice::Yes,
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains("not active"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mesh_is_mock_and_lists_karma() {
        let (dir, store) = temp_store();
        let mesh = store.mesh();
        assert_eq!(mesh.mode, "mock");
        assert_eq!(mesh.genesis_node_id, GENESIS_NODE_ID);
        let genesis = mesh
            .nodes
            .iter()
            .find(|n| n.node_id == GENESIS_NODE_ID)
            .expect("genesis");
        assert!(genesis.karma >= 500);
        let local = mesh
            .nodes
            .iter()
            .find(|n| n.node_id == "local")
            .expect("local");
        assert!(local.karma < 500);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn complete_inbox_awards_karma_to_claimer() {
        let (dir, store) = temp_store();
        let before = store
            .list_nodes()
            .into_iter()
            .find(|n| n.node_id == "local")
            .unwrap()
            .karma;
        store.claim("offer-seed-1", "local").unwrap();
        store.complete("offer-seed-1").unwrap();
        let after = store
            .list_nodes()
            .into_iter()
            .find(|n| n.node_id == "local")
            .unwrap()
            .karma;
        assert_eq!(after, before + 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn list_votes_filters_by_proposal() {
        let (dir, store) = temp_store();
        let other = store.create_proposal(CreateProposalBody {
            proposal_type: "general".into(),
            title: "Other".into(),
            body: String::new(),
            locked_param: None,
        });
        store
            .cast_vote(
                "p-genesis-params",
                CastVoteBody {
                    node_id: GENESIS_NODE_ID.into(),
                    choice: VoteChoice::Abstain,
                },
            )
            .unwrap();
        assert_eq!(store.list_votes(Some("p-genesis-params")).len(), 1);
        assert!(store.list_votes(Some(&other.id)).is_empty());
        assert_eq!(store.list_votes(None).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn claim_and_complete_are_idempotent() {
        let (dir, store) = temp_store();

        let err_unclaimed = store.complete("offer-seed-1").unwrap_err();
        assert!(err_unclaimed.to_string().contains("not claimed"));

        store.claim("offer-seed-1", "local").unwrap();
        store.claim("offer-seed-1", "local").unwrap();

        let err_claimed = store.claim("offer-seed-1", "other").unwrap_err();
        assert!(err_claimed.to_string().contains("already claimed"));

        store.complete("offer-seed-1").unwrap();

        let err_completed = store.complete("offer-seed-1").unwrap_err();
        assert!(err_completed.to_string().contains("already completed"));

        let _ = std::fs::remove_dir_all(dir);
    }

    fn support_body(title: &str) -> CreateSupportBody {
        CreateSupportBody {
            title: title.into(),
            body: "atomic persist test".into(),
            feature_id: None,
        }
    }

    #[test]
    fn persisted_store_json_is_valid_json_after_write() {
        let dir = std::env::temp_dir().join(format!("maloca-atomic-json-{}", short_id()));
        let store = MalocaStore::open(&dir);
        store.create_support(support_body("valid json ticket"));

        let raw = std::fs::read_to_string(dir.join("maloca").join("store.json")).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).expect("store.json must be valid JSON after a write");
        assert!(parsed.is_object());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn persist_leaves_no_tmp_files_after_success() {
        let dir = std::env::temp_dir().join(format!("maloca-atomic-tmp-{}", short_id()));
        let store = MalocaStore::open(&dir);
        store.create_support(support_body("first"));
        store.create_support(support_body("second"));

        let leftovers: Vec<String> = std::fs::read_dir(dir.join("maloca"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "successful persist must not leave temp files behind: {leftovers:?}"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// The store file must be replaced by rename, not rewritten in place. An
    /// in-place `fs::write` keeps the same inode, so a changed inode proves the
    /// temp-file-then-rename path ran.
    #[cfg(unix)]
    #[test]
    fn persist_replaces_store_file_by_rename_not_in_place() {
        use std::os::unix::fs::MetadataExt;

        let dir = std::env::temp_dir().join(format!("maloca-atomic-rename-{}", short_id()));
        let store = MalocaStore::open(&dir);
        let path = dir.join("maloca").join("store.json");

        store.create_support(support_body("first"));
        let inode_before = std::fs::metadata(&path).unwrap().ino();

        store.create_support(support_body("second"));
        let inode_after = std::fs::metadata(&path).unwrap().ino();

        assert_ne!(
            inode_before, inode_after,
            "store.json must be swapped in via rename; an in-place write keeps the inode"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    /// A write that cannot create its temp file must surface as an error from
    /// `persist()` rather than panicking.
    #[cfg(unix)]
    #[test]
    fn persist_into_read_only_directory_returns_error_without_panicking() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("maloca-atomic-ro-{}", short_id()));
        let maloca_dir = dir.join("maloca");
        std::fs::create_dir_all(&maloca_dir).unwrap();
        let store = MalocaStore::open(&dir);

        std::fs::set_permissions(&maloca_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let state = store.inner.read();
        let result = store.persist(&state);
        drop(state);
        assert!(
            result.is_err(),
            "persist into a read-only directory must return an error"
        );
        assert!(
            !maloca_dir.join("store.json").exists(),
            "a failed persist must not create store.json"
        );

        // Restore write permission so the tempdir can be cleaned up.
        std::fs::set_permissions(&maloca_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Adds an active peer above `vote_karma_min`, so two nodes are eligible.
    fn add_eligible_peer(store: &MalocaStore, node_id: &str) {
        store.inner.write().nodes.push(NodeRecord {
            node_id: node_id.into(),
            role: "peer".into(),
            karma: 600,
            active: true,
            note: String::new(),
        });
    }

    /// Makes an open proposal due by moving its expiry into the past.
    fn expire_proposal(store: &MalocaStore, proposal_id: &str) {
        let mut g = store.inner.write();
        let proposal = g
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .unwrap();
        proposal.expires_at = Some((Utc::now() - Duration::days(1)).to_rfc3339());
    }

    fn new_proposal(store: &MalocaStore, title: &str) -> Proposal {
        store.create_proposal(CreateProposalBody {
            proposal_type: "general".into(),
            title: title.into(),
            body: String::new(),
            locked_param: None,
        })
    }

    fn cast(
        store: &MalocaStore,
        proposal_id: &str,
        node_id: &str,
        choice: VoteChoice,
    ) -> Result<Vote> {
        store.cast_vote(
            proposal_id,
            CastVoteBody {
                node_id: node_id.into(),
                choice,
            },
        )
    }

    fn closed_decisions(store: &MalocaStore, proposal_id: &str) -> Vec<DecisionEvent> {
        store
            .list_decisions()
            .into_iter()
            .filter(|d| {
                d.kind == DecisionKind::Closed && d.proposal_id.as_deref() == Some(proposal_id)
            })
            .collect()
    }

    fn find_proposal(store: &MalocaStore, proposal_id: &str) -> Proposal {
        store
            .list_proposals()
            .into_iter()
            .find(|p| p.id == proposal_id)
            .expect("proposal present")
    }

    #[test]
    fn new_proposal_gets_expiry_from_duration_param() {
        let (dir, store) = temp_store();
        let p = new_proposal(&store, "expiry");
        let created = parse_timestamp(&p.created_at).unwrap();
        let expires = parse_timestamp(p.expires_at.as_deref().unwrap()).unwrap();
        assert_eq!(expires - created, Duration::days(7));
        assert!(p.outcome.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expired_proposal_with_quorum_closes_as_approved() {
        let (dir, store) = temp_store();
        add_eligible_peer(&store, "peer-a");
        let p = new_proposal(&store, "approve");
        // 1 of 2 eligible nodes voted: 50% participation, quorum 30% met.
        cast(&store, &p.id, GENESIS_NODE_ID, VoteChoice::Yes).unwrap();
        expire_proposal(&store, &p.id);

        let closed = find_proposal(&store, &p.id);
        assert_eq!(closed.status, ProposalStatus::Closed);
        assert_eq!(closed.outcome.as_deref(), Some("approved"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expired_proposal_with_quorum_and_majority_no_closes_as_rejected() {
        let (dir, store) = temp_store();
        add_eligible_peer(&store, "peer-a");
        let p = new_proposal(&store, "reject");
        cast(&store, &p.id, GENESIS_NODE_ID, VoteChoice::No).unwrap();
        expire_proposal(&store, &p.id);

        let closed = find_proposal(&store, &p.id);
        assert_eq!(closed.status, ProposalStatus::Closed);
        assert_eq!(closed.outcome.as_deref(), Some("rejected"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expired_proposal_without_quorum_closes_as_no_quorum() {
        let (dir, store) = temp_store();
        let p = new_proposal(&store, "silent");
        expire_proposal(&store, &p.id);

        let closed = find_proposal(&store, &p.id);
        assert_eq!(closed.status, ProposalStatus::Closed);
        assert_eq!(closed.outcome.as_deref(), Some("no_quorum"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn tie_is_rejected_at_store_level() {
        let (dir, store) = temp_store();
        add_eligible_peer(&store, "peer-a");
        let p = new_proposal(&store, "tie");
        cast(&store, &p.id, GENESIS_NODE_ID, VoteChoice::Yes).unwrap();
        cast(&store, &p.id, "peer-a", VoteChoice::No).unwrap();
        expire_proposal(&store, &p.id);

        assert_eq!(
            find_proposal(&store, &p.id).outcome.as_deref(),
            Some("rejected")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn not_yet_expired_proposal_stays_open() {
        let (dir, store) = temp_store();
        let p = new_proposal(&store, "still open");
        cast(&store, &p.id, GENESIS_NODE_ID, VoteChoice::Yes).unwrap();

        let open = find_proposal(&store, &p.id);
        assert_eq!(open.status, ProposalStatus::Open);
        assert!(open.outcome.is_none());
        assert!(store.close_due_proposals(Utc::now()).is_empty());
        assert!(closed_decisions(&store, &p.id).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn closing_appends_one_decision_and_is_idempotent() {
        let (dir, store) = temp_store();
        let p = new_proposal(&store, "close twice");
        expire_proposal(&store, &p.id);

        let first = store.close_due_proposals(Utc::now());
        assert_eq!(first.len(), 1);
        assert!(store.close_due_proposals(Utc::now()).is_empty());
        assert_eq!(closed_decisions(&store, &p.id).len(), 1);

        // The close survives a restart.
        let reopened = MalocaStore::open(&dir);
        let persisted = find_proposal(&reopened, &p.id);
        assert_eq!(persisted.status, ProposalStatus::Closed);
        assert_eq!(persisted.outcome.as_deref(), Some("no_quorum"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_proposal_without_expires_at_closes_by_created_plus_duration() {
        let dir = std::env::temp_dir().join(format!("maloca-legacy-expiry-{}", short_id()));
        let maloca_dir = dir.join("maloca");
        std::fs::create_dir_all(&maloca_dir).unwrap();
        let old = (Utc::now() - Duration::days(30)).to_rfc3339();
        let fresh = Utc::now().to_rfc3339();
        // No `expires_at` and no `outcome` keys: a store written before this change.
        let fixture = serde_json::json!({
            "support": [],
            "reviews": [],
            "inbox": [],
            "rewards": [],
            "manager_actions": [],
            "proposals": [
                { "id": "p-legacy-old", "type": "general", "title": "Old", "body": "",
                  "status": "open", "created_at": old },
                { "id": "p-legacy-fresh", "type": "general", "title": "Fresh", "body": "",
                  "status": "open", "created_at": fresh }
            ],
            "backlog": { "source": "fixture", "items": [] }
        });
        std::fs::write(maloca_dir.join("store.json"), fixture.to_string()).unwrap();

        let store = MalocaStore::open(&dir);
        let old_closed = find_proposal(&store, "p-legacy-old");
        assert_eq!(old_closed.status, ProposalStatus::Closed);
        assert_eq!(old_closed.outcome.as_deref(), Some("no_quorum"));
        assert_eq!(
            find_proposal(&store, "p-legacy-fresh").status,
            ProposalStatus::Open
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn vote_after_expiry_is_rejected() {
        let (dir, store) = temp_store();
        add_eligible_peer(&store, "peer-a");
        let p = new_proposal(&store, "late vote");
        expire_proposal(&store, &p.id);

        let err = cast(&store, &p.id, GENESIS_NODE_ID, VoteChoice::Yes).unwrap_err();
        assert!(err.to_string().contains("closed"), "got: {err}");
        assert!(store.list_votes(Some(&p.id)).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn support_ticket(store: &MalocaStore, title: &str) -> SupportTicket {
        store.create_support(support_body(title))
    }

    #[test]
    fn support_status_moves_forward_and_persists() {
        let (dir, store) = temp_store();
        let ticket = support_ticket(&store, "moves");
        for to in [
            SupportStatus::Triaged,
            SupportStatus::InProgress,
            SupportStatus::Resolved,
            SupportStatus::Closed,
        ] {
            let updated = store.update_support_status(&ticket.id, to).unwrap();
            assert_eq!(updated.status, support_status_name(to));
        }

        let reopened = MalocaStore::open(&dir);
        assert_eq!(reopened.list_support()[0].status, "closed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn support_invalid_transition_is_reported_and_leaves_status() {
        let (dir, store) = temp_store();
        let ticket = support_ticket(&store, "invalid");
        let err = store
            .update_support_status(&ticket.id, SupportStatus::Closed)
            .unwrap_err();
        assert!(err
            .downcast_ref::<crate::maloca::support_flow::InvalidTransition>()
            .is_some());
        assert_eq!(store.list_support()[0].status, "open");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn support_unknown_id_is_not_an_invalid_transition() {
        let (dir, store) = temp_store();
        let err = store
            .update_support_status("s-missing", SupportStatus::Triaged)
            .unwrap_err();
        assert!(err
            .downcast_ref::<crate::maloca::support_flow::InvalidTransition>()
            .is_none());
        assert!(err.to_string().contains("not found"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn support_unknown_stored_status_is_treated_as_open() {
        let (dir, store) = temp_store();
        let ticket = support_ticket(&store, "legacy");
        store.inner.write().support[0].status = "something-else".into();
        let updated = store
            .update_support_status(&ticket.id, SupportStatus::Triaged)
            .unwrap();
        assert_eq!(updated.status, "triaged");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn support_status_names_round_trip() {
        for status in [
            SupportStatus::Open,
            SupportStatus::Triaged,
            SupportStatus::InProgress,
            SupportStatus::Resolved,
            SupportStatus::Closed,
        ] {
            assert_eq!(
                parse_support_status(support_status_name(status)),
                Some(status)
            );
        }
        assert_eq!(parse_support_status("bogus"), None);
    }
}
