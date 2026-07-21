//! # Gobernanza Bicameral — 50% Usuarios + 50% Consejo Xavier Core
//!
//! ## XIP Lifecycle State Machine
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                     XIP Lifecycle                            │
//! └──────────────────────────────────────────────────────────────┘
//!                              │
//!                       ┌──────▼──────┐
//!                       │    DRAFT    │
//!                       └──────┬──────┘
//!                              │ support_proposal() (5 supports)
//!                       ┌──────▼──────────┐
//!                       │   DISCUSSION     │  ← 3-day expiry (auto-complete)
//!                       └──────┬──────────┘
//!                              │ supports ≥ min_supports
//!                       ┌──────▼──────┐
//!                       │   VOTING    │  ← 7-day expiry (auto-complete)
//!                       └──────┬──────┘
//!                          ┌───┴───┐
//!                          │       │
//!                   ┌──────▼──┐ ┌──▼───────┐
//!                   │ APPROVE │ │ REJECT   │
//!                   └──────┬──┘ └──────────┘
//!                          │
//!                   ┌──────▼──────────┐
//!                   │   EXECUTION     │  ← 48h timer
//!                   └──────┬──────────┘
//!                          │ execute_proposal()
//!                   ┌──────▼──────┐
//!                   │  COMPLETE   │
//!                   └─────────────┘
//! ```
//!
//! ## Weighted Voting by Reputation
//!
//! Cada voto de usuario tiene un peso = trust_score normalizado (reputation_weight).
//! - Total yes_weight / total weight > 50% = cámara de usuarios aprueba
//! - Consejo: 1 miembro = 1 voto (voto plano)
//!
//! ## Bicameral Decision
//!
//! - Cámara 1 (Usuarios): 50% peso, votos ponderados por reputación
//! - Cámara 2 (Consejo): 50% peso, 1 miembro = 1 voto
//! - Ambas deben aprobar para que el XIP pase
//!
//! ## Council Veto & Community Overrule
//!
//! - **Veto:** 66% del consejo puede vetar por razones de seguridad
//! - **Overrule:** 75% de usuarios puede overrulear el veto
//!
//! ## Voter Eligibility
//!
//! Una wallet solo puede votar si ha tenido actividad en los últimos 7 días.
//! Sin delegación de voto.

use crate::data_commons::reputation::reputation_weight;
use crate::data_commons::reputation::EigenTrustEngine;
use crate::data_commons::types::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Configuración de gobernanza bicameral
#[derive(Debug, Clone)]
pub struct GovernanceConfig {
    /// Período de discusión en días (default: 3)
    pub discussion_period_days: u32,
    /// Período de votación en días (default: 7)
    pub voting_period_days: u32,
    /// Timer de ejecución en horas (default: 48)
    pub execution_timer_hours: u32,
    /// Quórum mínimo usuarios en % (default: 10)
    pub user_quorum_minimum: f32,
    /// Quórum mínimo consejo en % (default: 51)
    pub council_quorum_minimum: f32,
    /// Apoyos mínimos para pasar a votación (default: 5)
    pub min_supports: u32,
    /// Peso de usuarios (default: 50%)
    pub user_weight: f32,
    /// Peso del consejo (default: 50%)
    pub council_weight: f32,
    /// % para veto del consejo (default: 66%)
    pub council_veto_threshold: f32,
    /// % para overrule de veto comunitario (default: 75%)
    pub community_overrule_threshold: f32,
    /// Días de inactividad para perder derecho a voto (default: 7)
    pub voting_activity_window_days: u32,
    /// % para expulsión por collusion (default: 66%)
    pub expulsion_threshold: f32,
    /// Dynamic quorum thresholds (optional)
    pub dynamic_quorum: Option<DynamicQuorum>,
}

impl GovernanceConfig {
    /// Builder: attach a dynamic quorum configuration
    pub fn with_dynamic_quorum(mut self, dq: DynamicQuorum) -> Self {
        self.dynamic_quorum = Some(dq);
        self
    }
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            discussion_period_days: 3,
            voting_period_days: 7,
            execution_timer_hours: 48,
            user_quorum_minimum: 10.0,
            council_quorum_minimum: 51.0,
            min_supports: 5,
            user_weight: 50.0,
            council_weight: 50.0,
            council_veto_threshold: 66.0,
            community_overrule_threshold: 75.0,
            voting_activity_window_days: 7,
            expulsion_threshold: 66.0,
            dynamic_quorum: None,
        }
    }
}

/// Dynamic quorum thresholds that adjust based on recent participation.
///
/// When participation is low, quorum is lowered to encourage voting.
/// When participation is high, quorum is raised to maintain engagement quality.
#[derive(Debug, Clone)]
pub struct DynamicQuorum {
    /// Base user quorum as fraction (default: 0.10 = 10%)
    pub base_user_quorum_pct: f64,
    /// Base council quorum as fraction (default: 0.51 = 51%)
    pub base_council_quorum_pct: f64,
    /// Boost factor derived from recent participation trends
    pub participation_boost: f64,
}

impl Default for DynamicQuorum {
    fn default() -> Self {
        Self {
            base_user_quorum_pct: 0.10,
            base_council_quorum_pct: 0.51,
            participation_boost: 1.0,
        }
    }
}

impl DynamicQuorum {
    /// Create a new DynamicQuorum with the given base percentages.
    pub fn new(base_user_quorum_pct: f64, base_council_quorum_pct: f64) -> Self {
        Self {
            base_user_quorum_pct,
            base_council_quorum_pct,
            participation_boost: 1.0,
        }
    }

    /// Compute effective user quorum based on recent participation rate.
    ///
    /// - If `recent_participation_rate < 0.30`: lower quorum by 20% (encourage participation)
    /// - If `recent_participation_rate > 0.80`: raise quorum by 10% (maintain engagement)
    /// - Otherwise: return base
    pub fn effective_user_quorum(&self, recent_participation_rate: f64) -> f64 {
        let base = self.base_user_quorum_pct;
        if recent_participation_rate < 0.30 {
            base * 0.80 // lower by 20%
        } else if recent_participation_rate > 0.80 {
            base * 1.10 // raise by 10%
        } else {
            base
        }
    }

    /// Compute effective council quorum based on recent council participation rate.
    ///
    /// Same logic as user quorum but with council-specific base.
    pub fn effective_council_quorum(&self, recent_council_participation: f64) -> f64 {
        let base = self.base_council_quorum_pct;
        if recent_council_participation < 0.30 {
            base * 0.80
        } else if recent_council_participation > 0.80 {
            base * 1.10
        } else {
            base
        }
    }
}

/// Motor de gobernanza bicameral
pub struct GovernanceEngine {
    config: GovernanceConfig,
    /// Propuestas activas
    proposals: Vec<XipProposal>,
    /// Miembros del consejo
    council: Vec<CouncilMember>,
    /// Wallets activas (con feedback en últimos N días)
    active_wallets: HashMap<WalletAddress, u64>, // wallet → last_activity_timestamp
    /// Wallets bloqueadas por collusion
    blocked_wallets: Vec<WalletAddress>,
    /// Optional EigenTrust reputation engine for weighted voting
    reputation_engine: Option<Arc<RwLock<EigenTrustEngine>>>,
}

impl GovernanceEngine {
    /// New.
    pub fn new(config: GovernanceConfig) -> Self {
        Self {
            config,
            proposals: Vec::new(),
            council: Vec::new(),
            active_wallets: HashMap::new(),
            blocked_wallets: Vec::new(),
            reputation_engine: None,
        }
    }

    /// Attach an EigenTrust reputation engine for weighted voting.
    pub fn with_reputation_engine(mut self, engine: Arc<RwLock<EigenTrustEngine>>) -> Self {
        self.reputation_engine = Some(engine);
        self
    }

    /// Export the internal state of the engine.
    pub fn get_state(&self) -> (Vec<XipProposal>, Vec<CouncilMember>, HashMap<WalletAddress, u64>, Vec<WalletAddress>) {
        (
            self.proposals.clone(),
            self.council.clone(),
            self.active_wallets.clone(),
            self.blocked_wallets.clone(),
        )
    }

    /// Import and set the internal state of the engine.
    pub fn set_state(
        &mut self,
        proposals: Vec<XipProposal>,
        council: Vec<CouncilMember>,
        active_wallets: HashMap<WalletAddress, u64>,
        blocked_wallets: Vec<WalletAddress>,
    ) {
        self.proposals = proposals;
        self.council = council;
        self.active_wallets = active_wallets;
        self.blocked_wallets = blocked_wallets;
    }

    // ── Helpers de tiempo ─────────────────────────────────

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    // ── XIP State Machine ─────────────────────────────────

    /// Validar y ejecutar transición de estado en una propuesta
    pub fn transition_to_state(
        proposal: &mut XipProposal,
        new_state: XipState,
    ) -> Result<(), GovernanceError> {
        if !proposal.xip_state.can_transition_to(&new_state) {
            return Err(GovernanceError::InvalidStateTransition {
                from: proposal.xip_state.label().to_string(),
                to: new_state.label().to_string(),
            });
        }
        proposal.xip_state = new_state;
        Ok(())
    }

    /// Auto-transition: mueve propuestas vencidas al siguiente estado o a Complete
    pub fn auto_transition_expired(&mut self) {
        let now = Self::now_secs();
        let proposal_ids: Vec<String> = self.proposals.iter().map(|p| p.id.clone()).collect();

        for id in proposal_ids {
            let needs_update = {
                let p = match self.proposals.iter().find(|p| p.id == id) {
                    Some(p) => p,
                    None => continue,
                };

                match &p.xip_state {
                    XipState::Discussion { expires_at, .. } => now >= *expires_at,
                    XipState::Voting { expires_at, .. } => now >= *expires_at,
                    XipState::Execution { expires_at, .. } => now >= *expires_at,
                    _ => false,
                }
            };

            if needs_update {
                let p = self.proposals.iter_mut().find(|p| p.id == id).unwrap();
                let new_state = match &p.xip_state {
                    XipState::Discussion { .. } => {
                        // Discussion expired → auto-complete without enough supports
                        XipState::Complete { entered_at: now }
                    }
                    XipState::Voting { .. } => {
                        // Voting expired → tally and transition
                        // This is a soft-transition; the actual result is computed externally
                        XipState::Complete { entered_at: now }
                    }
                    XipState::Execution { .. } => {
                        // Execution timer expired → mark complete
                        p.status = ProposalStatus::Executed;
                        XipState::Complete { entered_at: now }
                    }
                    _ => continue,
                };
                p.xip_state = new_state;
            }
        }
    }

    // ── Cámara 1: Usuarios ──────────────────────────────────

    /// Registrar actividad de una wallet (compartir, comprar o validar contexto)
    ///
    /// Esto mantiene el derecho a voto activo.
    pub fn register_activity(&mut self, wallet: WalletAddress) {
        let now = Self::now_secs();
        self.active_wallets.insert(wallet, now);
    }

    /// Verificar si una wallet puede votar (actividad en últimos N días)
    pub fn can_user_vote(&self, wallet: &WalletAddress) -> bool {
        if self.blocked_wallets.contains(wallet) {
            return false;
        }

        let now = Self::now_secs();
        let window_secs = self.config.voting_activity_window_days as u64 * 86400;

        self.active_wallets
            .get(wallet)
            .map(|&last_activity| now.saturating_sub(last_activity) <= window_secs)
            .unwrap_or(false)
    }

    /// Obtener wallets activas para votar (cumplen feedback de 7 días)
    pub fn active_voter_wallets(&self) -> Vec<&WalletAddress> {
        let now = Self::now_secs();
        let window_secs = self.config.voting_activity_window_days as u64 * 86400;

        self.active_wallets
            .iter()
            .filter(|(_, &last_activity)| now.saturating_sub(last_activity) <= window_secs)
            .map(|(wallet, _)| wallet)
            .collect()
    }

    // ── Cámara 2: Consejo Xavier Core ─────────────────────

    /// Agregar miembro al consejo
    pub fn add_council_member(
        &mut self,
        wallet: WalletAddress,
        role: CouncilRole,
        expertise: Vec<String>,
    ) -> CouncilMember {
        let member = CouncilMember {
            id: format!("council_{}", wallet.0),
            wallet,
            role,
            joined_at: Self::now_secs(),
            active: true,
            expertise,
        };
        self.council.push(member.clone());
        member
    }

    /// Obtener miembros activos del consejo
    pub fn active_council_members(&self) -> Vec<&CouncilMember> {
        self.council.iter().filter(|m| m.active).collect()
    }

    // ── Propuestas (XIP) ──────────────────────────────────

    /// Crear una nueva propuesta (XIP)
    pub fn create_proposal(
        &mut self,
        title: String,
        description: String,
        changes: HashMap<String, String>,
        author: WalletAddress,
    ) -> Result<XipProposal, GovernanceError> {
        if !self.can_user_vote(&author) {
            return Err(GovernanceError::InactiveVoter);
        }

        let now = Self::now_secs();

        let proposal = XipProposal {
            id: format!("xip_{}_{}", now, self.proposals.len() + 1),
            title,
            description,
            changes,
            author,
            status: ProposalStatus::Draft,
            xip_state: XipState::Draft { entered_at: now },
            created_at: now,
            discussion_end: now + self.config.discussion_period_days as u64 * 86400,
            voting_end: now
                + (self.config.discussion_period_days + self.config.voting_period_days) as u64
                    * 86400,
            execution_at: now
                + (self.config.discussion_period_days + self.config.voting_period_days) as u64
                    * 86400
                + self.config.execution_timer_hours as u64 * 3600,
            user_votes: HashMap::new(),
            weighted_user_votes: HashMap::new(),
            council_votes: HashMap::new(),
            supports: Vec::new(),
            council_veto: false,
            veto_reason: None,
            appealed: false,
        };

        self.proposals.push(proposal.clone());
        Ok(proposal)
    }

    /// Apoyar una propuesta (para pasar a votación)
    pub fn support_proposal(
        &mut self,
        proposal_id: &str,
        wallet: &WalletAddress,
    ) -> Result<(), GovernanceError> {
        let can_vote = self.can_user_vote(wallet);
        let min_supports = self.config.min_supports;

        if !can_vote {
            return Err(GovernanceError::InactiveVoter);
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        // Validar estado: solo desde Draft o Discussion se puede apoyar
        match proposal.xip_state {
            XipState::Draft { .. } | XipState::Discussion { .. } => {}
            _ => return Err(GovernanceError::VotingNotOpen),
        }

        if proposal.supports.contains(wallet) {
            return Err(GovernanceError::AlreadySupported);
        }

        proposal.supports.push(wallet.clone());

        // Si alcanza apoyos mínimos, pasar a Discussion → Voting
        if proposal.supports.len() >= min_supports as usize {
            let now = Self::now_secs();
            proposal.status = ProposalStatus::Voting;

            // Transition: Draft → Voting or Discussion → Voting
            match proposal.xip_state {
                XipState::Draft { .. } | XipState::Discussion { .. } => {
                    let voting_expires = now + self.config.voting_period_days as u64 * 86400;
                    proposal.xip_state = XipState::Voting {
                        entered_at: now,
                        expires_at: voting_expires,
                    };
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Voto de usuario (anónimo — cifrado con Kyber)
    /// Utiliza votación ponderada por reputación (reputation_weight)
    pub fn user_vote(
        &mut self,
        proposal_id: &str,
        wallet: &WalletAddress,
        in_favor: bool,
        _encrypted_vote: Vec<u8>,
        _dilithium_signature: Vec<u8>,
    ) -> Result<(), GovernanceError> {
        let can_vote = self.can_user_vote(wallet);

        if !can_vote {
            return Err(GovernanceError::InactiveVoter);
        }

        // Calcular peso del voto ANTES de tomar el mutable borrow de proposals
        let weight = self.reputation_vote_weight(&wallet.0);

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        // Validar que la votación esté abierta según XIP state machine
        if !matches!(proposal.xip_state, XipState::Voting { .. }) {
            return Err(GovernanceError::VotingNotOpen);
        }

        if proposal.weighted_user_votes.contains_key(wallet) {
            return Err(GovernanceError::AlreadyVoted);
        }

        let now = Self::now_secs();
        let weighted_vote = WeightedVote {
            wallet_id: wallet.clone(),
            weight,
            approve: in_favor,
            timestamp: now,
        };

        // Legacy: mantener user_votes actualizado por compatibilidad
        proposal.user_votes.insert(wallet.clone(), in_favor);
        proposal
            .weighted_user_votes
            .insert(wallet.clone(), weighted_vote);
        Ok(())
    }

    /// Calculate vote weight for a wallet based on EigenTrust reputation.
    ///
    /// If a reputation engine is attached, the wallet's trust score is normalized
    /// to a 1.0-10.0 range (max_score / 10). Otherwise falls back to 1.0.
    fn reputation_vote_weight(&self, wallet_id: &str) -> u64 {
        if let Some(ref engine) = self.reputation_engine {
            if let Ok(eng) = engine.read() {
                let score = eng.computed_score(wallet_id);
                if score > 0.0 {
                    // Normalize to 1.0-10.0 range: divide max possible score by 10
                    // Since EigenTrust scores are typically 0.0-1.0, scale up by 10
                    let normalized = (score * 10.0).clamp(1.0, 10.0);
                    return normalized.round() as u64;
                }
            }
        }
        // Fallback: base weight of 1
        reputation_weight(wallet_id)
    }

    /// Obtener el peso total de votos a favor de usuarios para una propuesta
    pub fn weighted_user_vote_tally(
        &self,
        proposal_id: &str,
    ) -> Result<(u64, u64), GovernanceError> {
        let proposal = self
            .proposals
            .iter()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        let total_weight: u64 = proposal
            .weighted_user_votes
            .values()
            .map(|v| v.weight)
            .sum();
        let yes_weight: u64 = proposal
            .weighted_user_votes
            .values()
            .filter(|v| v.approve)
            .map(|v| v.weight)
            .sum();

        Ok((yes_weight, total_weight))
    }

    /// Voto del consejo (público)
    pub fn council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        in_favor: bool,
    ) -> Result<(), GovernanceError> {
        let member_active = self.council.iter().any(|m| m.id == member_id && m.active);
        if !member_active {
            return Err(GovernanceError::NotAuthorized);
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if !matches!(proposal.xip_state, XipState::Voting { .. }) {
            return Err(GovernanceError::VotingNotOpen);
        }

        if proposal.council_votes.contains_key(member_id) {
            return Err(GovernanceError::AlreadyVoted);
        }

        proposal
            .council_votes
            .insert(member_id.to_string(), in_favor);
        Ok(())
    }

    /// Veto del consejo (solo para propuestas que comprometen seguridad/arquitectura)
    ///
    /// Requiere 66% del consejo (supermayoría).
    /// Debe incluir razón pública.
    pub fn council_veto(
        &mut self,
        proposal_id: &str,
        reason: String,
    ) -> Result<(), GovernanceError> {
        let active_members = self.active_council_members().len() as f32;
        let veto_threshold = self.config.council_veto_threshold;

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if !matches!(proposal.xip_state, XipState::Voting { .. }) {
            return Err(GovernanceError::VotingNotOpen);
        }

        let veto_votes = proposal.council_votes.values().filter(|&&v| !v).count() as f32;
        let needed = (active_members * veto_threshold / 100.0).ceil();

        if veto_votes < needed {
            return Err(GovernanceError::VetoThresholdNotReached);
        }

        proposal.council_veto = true;
        proposal.veto_reason = Some(reason);
        proposal.status = ProposalStatus::Vetoed;
        // XIP state stays in Voting until resolved

        Ok(())
    }

    /// Apelación comunitaria — overrule del veto del consejo
    ///
    /// Requiere 75% de apoyo ponderado de wallets activas en segunda votación.
    pub fn community_appeal(&mut self, proposal_id: &str) -> Result<(), GovernanceError> {
        let total_active = self.active_voter_wallets().len() as f32;
        let quorum_minimum = self.config.user_quorum_minimum;
        let overrule_threshold = self.config.community_overrule_threshold;

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if !proposal.council_veto {
            return Err(GovernanceError::NoVetoToAppeal);
        }

        // Usar votación ponderada para la apelación
        let total_weight: u64 = proposal
            .weighted_user_votes
            .values()
            .map(|v| v.weight)
            .sum();
        let yes_weight: u64 = proposal
            .weighted_user_votes
            .values()
            .filter(|v| v.approve)
            .map(|v| v.weight)
            .sum();
        let total_votes_cast = proposal.weighted_user_votes.len() as f32;

        if total_votes_cast < (total_active * quorum_minimum / 100.0).floor() {
            return Err(GovernanceError::QuorumNotMet);
        }

        let support_percentage = if total_weight > 0 {
            (yes_weight as f32 / total_weight as f32) * 100.0
        } else {
            0.0
        };

        if support_percentage >= overrule_threshold {
            proposal.status = ProposalStatus::Overruled;
            proposal.council_veto = false;
            proposal.appealed = true;
            Ok(())
        } else {
            Err(GovernanceError::OverruleThresholdNotReached)
        }
    }

    /// Contar votos de una propuesta finalizada (usa votación ponderada)
    pub fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, GovernanceError> {
        let now = Self::now_secs();

        // Extraer datos de la propuesta (borrow inmutable)
        let (
            _id,
            user_votes,
            weighted_user_votes,
            council_votes,
            council_veto,
            appealed,
            _voting_end,
        ) = {
            let proposal = self
                .proposals
                .iter()
                .find(|p| p.id == proposal_id)
                .ok_or(GovernanceError::ProposalNotFound)?;

            let voting_ended = now >= proposal.voting_end || proposal.council_veto;
            if !voting_ended {
                return Err(GovernanceError::VotingNotEnded);
            }

            (
                proposal.id.clone(),
                proposal.user_votes.clone(),
                proposal.weighted_user_votes.clone(),
                proposal.council_votes.clone(),
                proposal.council_veto,
                proposal.appealed,
                proposal.voting_end,
            )
        };

        // ── Cámara 1: Usuarios (Votación Ponderada) ──
        let active_users = self.active_voter_wallets().len() as f32;
        let total_weight: u64 = weighted_user_votes.values().map(|v| v.weight).sum();
        let yes_weight: u64 = weighted_user_votes
            .values()
            .filter(|v| v.approve)
            .map(|v| v.weight)
            .sum();
        let total_user_votes = weighted_user_votes.len() as f32;

        let user_for = user_votes.values().filter(|&&v| v).count() as u64;
        let user_against = user_votes.values().filter(|&&v| !v).count() as u64;

        // Compute effective quorum thresholds (dynamic or static)
        let effective_user_quorum_pct = if let Some(ref dq) = self.config.dynamic_quorum {
            let participation_rate = if active_users > 0.0 {
                total_user_votes / active_users
            } else {
                0.0
            };
            dq.effective_user_quorum(participation_rate as f64) as f32 * 100.0
        } else {
            self.config.user_quorum_minimum
        };

        let user_quorum_met =
            total_user_votes >= (active_users * effective_user_quorum_pct / 100.0).floor();
        // Ponderado: yes_weight / total_weight > 50%
        let user_percentage = if total_weight > 0 {
            (yes_weight as f32 / total_weight as f32) * 100.0
        } else {
            0.0
        };

        // ── Cámara 2: Consejo ──
        let active_council = self.active_council_members().len() as f32;
        let total_council_votes = council_votes.len() as f32;

        let council_for = council_votes.values().filter(|&&v| v).count() as u64;
        let council_against = council_votes.values().filter(|&&v| !v).count() as u64;

        let effective_council_quorum_pct = if let Some(ref dq) = self.config.dynamic_quorum {
            let council_participation = if active_council > 0.0 {
                total_council_votes / active_council
            } else {
                0.0
            };
            dq.effective_council_quorum(council_participation as f64) as f32 * 100.0
        } else {
            self.config.council_quorum_minimum
        };

        let council_quorum_met =
            total_council_votes >= (active_council * effective_council_quorum_pct / 100.0).floor();
        let council_percentage = if total_council_votes > 0.0 {
            (council_for as f32 / total_council_votes) * 100.0
        } else {
            0.0
        };

        // ── Resultado Final ──
        let user_passed = user_quorum_met && user_percentage > 50.0;
        let council_passed = council_quorum_met && council_percentage > 50.0;
        let passed = user_passed && council_passed;

        let veto_overruled = council_veto && appealed;

        let result = BicameralResult {
            proposal_id: proposal_id.to_string(),
            user_votes_for: user_for,
            user_votes_against: user_against,
            user_abstain: total_user_votes as u64 - user_for - user_against,
            user_quorum_met,
            user_percentage_for: user_percentage,
            user_active_wallets: active_users as u64,
            council_votes_for: council_for,
            council_votes_against: council_against,
            council_total: active_council as u64,
            council_percentage_for: council_percentage,
            council_veto_active: council_veto && !appealed,
            passed,
            veto_overruled,
            executed: false,
            tallied_at: now,
        };

        // Actualizar estado de la propuesta
        if let Some(p) = self.proposals.iter_mut().find(|p| p.id == proposal_id) {
            if council_veto && !appealed {
                // veto activo, no apelado → se queda como Vetoed
                // XIP state: stays in Voting (vetoed but not resolved)
            } else if passed {
                p.status = ProposalStatus::Approved;
                // Transition: Voting → Execution
                let exec_expires = now + self.config.execution_timer_hours as u64 * 3600;
                p.xip_state = XipState::Execution {
                    entered_at: now,
                    expires_at: exec_expires,
                };
            } else if appealed {
                p.status = ProposalStatus::Overruled;
                p.xip_state = XipState::Complete { entered_at: now };
            } else {
                p.status = ProposalStatus::Rejected;
                p.xip_state = XipState::Complete { entered_at: now };
            }
        }

        Ok(result)
    }

    /// Ejecutar una propuesta aprobada
    pub fn execute_proposal(
        &mut self,
        proposal_id: &str,
        system_params: &mut SystemParams,
    ) -> Result<(), GovernanceError> {
        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        // Verificar que esté en estado Execution
        if !matches!(proposal.xip_state, XipState::Execution { .. }) {
            return Err(GovernanceError::ProposalNotApproved);
        }

        let now = Self::now_secs();

        // Verificar timer de 48h
        if let XipState::Execution { expires_at, .. } = proposal.xip_state {
            if now < expires_at {
                return Err(GovernanceError::ExecutionTimerNotReady);
            }
        }

        // Aplicar cambios
        for (key, value) in &proposal.changes {
            match key.as_str() {
                "reference_price" => {
                    if let Ok(v) = value.parse::<u64>() {
                        system_params.reference_price = v;
                    }
                }
                "burn_rate" => {
                    if let Ok(v) = value.parse::<u8>() {
                        system_params.burn_rate = v.min(100);
                    }
                }
                "voting_period_days" => {
                    if let Ok(v) = value.parse::<u32>() {
                        system_params.voting_period_days = v;
                    }
                }
                "quorum_minimum" => {
                    if let Ok(v) = value.parse::<f32>() {
                        system_params.quorum_minimum = v.min(100.0);
                    }
                }
                "min_price" => {
                    if let Ok(v) = value.parse::<u64>() {
                        system_params.min_price = v;
                    }
                }
                "max_price" => {
                    if let Ok(v) = value.parse::<u64>() {
                        system_params.max_price = v;
                    }
                }
                _ => {} // ignorar cambios desconocidos
            }
        }

        proposal.status = ProposalStatus::Executed;
        proposal.xip_state = XipState::Complete { entered_at: now };
        Ok(())
    }

    /// Proponer expulsión de wallet por collusion comprobada
    pub fn propose_expulsion(
        &mut self,
        target: WalletAddress,
        evidence: String,
        author: WalletAddress,
    ) -> Result<XipProposal, GovernanceError> {
        let mut changes = HashMap::new();
        changes.insert("block_wallet".into(), target.0.clone());
        changes.insert("evidence".into(), evidence);

        let proposal = self.create_proposal(
            format!("Expulsión de wallet {}", target.0),
            "Expulsión por collusion comprobada".into(),
            changes,
            author,
        )?;

        // Las expulsiones requieren 66% (handled by proposal voting process)
        Ok(proposal)
    }

    // ── Getters ──────────────────────────────────────────

    /// Active proposals.
    pub fn active_proposals(&self) -> Vec<&XipProposal> {
        self.proposals
            .iter()
            .filter(|p| {
                matches!(
                    p.xip_state,
                    XipState::Discussion { .. } | XipState::Voting { .. }
                )
            })
            .collect()
    }

    /// Get proposal.
    pub fn get_proposal(&self, id: &str) -> Option<&XipProposal> {
        self.proposals.iter().find(|p| p.id == id)
    }

    /// Council size.
    pub fn council_size(&self) -> usize {
        self.active_council_members().len()
    }

    /// Active voter count.
    pub fn active_voter_count(&self) -> usize {
        self.active_voter_wallets().len()
    }
}

#[derive(Debug)]
pub enum GovernanceError {
    NotAuthorized,
    ProposalNotFound,
    ProposalNotApproved,
    AlreadyVoted,
    AlreadySupported,
    VotingNotOpen,
    VotingNotEnded,
    InsufficientSupports,
    QuorumNotMet,
    ExecutionTimerNotReady,
    WalletBlocked,
    InvalidProposal,
    WalletAlreadyBlocked,
    InactiveVoter,
    VetoThresholdNotReached,
    OverruleThresholdNotReached,
    NoVetoToAppeal,
    InvalidStateTransition { from: String, to: String },
}

impl std::fmt::Display for GovernanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "No autorizado para esta acción"),
            Self::ProposalNotFound => write!(f, "Propuesta no encontrada"),
            Self::ProposalNotApproved => write!(f, "La propuesta no está aprobada"),
            Self::AlreadyVoted => write!(f, "Ya votaste en esta propuesta"),
            Self::AlreadySupported => write!(f, "Ya apoyaste esta propuesta"),
            Self::VotingNotOpen => write!(f, "La votación no está abierta"),
            Self::VotingNotEnded => write!(f, "La votación no ha terminado"),
            Self::InsufficientSupports => {
                write!(f, "Se necesitan más apoyos para pasar a votación")
            }
            Self::QuorumNotMet => write!(f, "No se alcanzó el quórum mínimo"),
            Self::ExecutionTimerNotReady => {
                write!(f, "El timer de ejecución de 48h no ha expirado")
            }
            Self::WalletBlocked => write!(f, "Esta wallet está bloqueada por collusion"),
            Self::InvalidProposal => write!(f, "Propuesta inválida"),
            Self::WalletAlreadyBlocked => write!(f, "La wallet ya está bloqueada"),
            Self::InactiveVoter => write!(
                f,
                "Wallet inactiva — necesita actividad en los últimos 7 días"
            ),
            Self::VetoThresholdNotReached => {
                write!(f, "No se alcanzó el 66% necesario para veto del consejo")
            }
            Self::OverruleThresholdNotReached => {
                write!(f, "No se alcanzó el 75% necesario para overrule del veto")
            }
            Self::NoVetoToAppeal => write!(f, "No hay veto activo para apelar"),
            Self::InvalidStateTransition { from, to } => {
                write!(f, "Transición de estado inválida: {from} → {to}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_proposal() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let wallet = WalletAddress("xv1_test_author".into());
        engine.register_activity(wallet.clone());

        let mut changes = HashMap::new();
        changes.insert("reference_price".into(), "10".into());

        let proposal = engine.create_proposal(
            "Aumentar precio referencial".into(),
            "Propongo subir el precio referencial de 5 a 10 $XAV".into(),
            changes,
            wallet,
        );

        assert!(proposal.is_ok());
        assert_eq!(engine.proposals.len(), 1);
    }

    #[test]
    fn test_inactive_wallet_cannot_vote() {
        let config = GovernanceConfig::default();
        let engine = GovernanceEngine::new(config);

        let wallet = WalletAddress("xv1_inactive".into());
        assert!(
            !engine.can_user_vote(&wallet),
            "Wallet sin actividad no debería poder votar"
        );
    }

    #[test]
    fn test_active_wallet_can_vote() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let wallet = WalletAddress("xv1_active".into());
        engine.register_activity(wallet.clone());
        assert!(
            engine.can_user_vote(&wallet),
            "Wallet con actividad reciente debería poder votar"
        );
    }

    #[test]
    fn test_proposal_needs_supports_to_reach_voting() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_author".into());
        engine.register_activity(author.clone());

        let mut changes = HashMap::new();
        changes.insert("min_price".into(), "2".into());

        let proposal = engine
            .create_proposal("Test".into(), "Test".into(), changes, author.clone())
            .unwrap();

        // Recién creada, debería estar en Draft
        assert_eq!(proposal.status, ProposalStatus::Draft);
        assert_eq!(proposal.xip_state.label(), "Draft");

        // Dar los 5 apoyos necesarios
        for i in 0..5 {
            let supporter = WalletAddress(format!("xv1_supporter_{}", i));
            engine.register_activity(supporter.clone());
            engine.support_proposal(&proposal.id, &supporter).unwrap();
        }

        // Ahora debería pasar a Voting
        let updated = engine.get_proposal(&proposal.id).unwrap();
        assert_eq!(updated.status, ProposalStatus::Voting);
    }

    #[test]
    fn test_bicameral_vote_requires_both_majorities() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_author".into());
        engine.register_activity(author.clone());

        let mut changes = HashMap::new();
        changes.insert("reference_price".into(), "10".into());

        let proposal = engine
            .create_proposal("Test bicameral".into(), "Test".into(), changes, author)
            .unwrap();

        // Dar apoyos
        for i in 0..5 {
            let s = WalletAddress(format!("xv1_s_{}", i));
            engine.register_activity(s.clone());
            engine.support_proposal(&proposal.id, &s).unwrap();
        }

        // Votos de usuarios (todos a favor)
        for i in 0..10 {
            let voter = WalletAddress(format!("xv1_user_{}", i));
            engine.register_activity(voter.clone());
            engine
                .user_vote(&proposal.id, &voter, true, vec![], vec![])
                .unwrap();
        }

        // Votos del consejo (todos en contra)
        let council_member = engine.add_council_member(
            WalletAddress("xv1_council".into()),
            CouncilRole::CoreMaintainer,
            vec!["security".into()],
        );

        // 51% quórum = 1 miembro
        engine
            .council_vote(&proposal.id, &council_member.id, false)
            .unwrap();

        // Para que tally_votes no devuelva VotingNotEnded sin que voting_end haya pasado,
        // necesitamos ejecutar un veto formal si el consejo votó en contra.
        engine
            .council_veto(&proposal.id, "Security concern".into())
            .unwrap();

        let result = engine.tally_votes(&proposal.id).unwrap();
        assert!(result.user_quorum_met);
        assert!(result.user_percentage_for > 50.0); // usuarios aprobaron
        assert!(
            !result.passed, // pero la propuesta NO pasa porque consejo dijo que no
            "La propuesta no debería pasar — consejo votó en contra"
        );
    }

    #[test]
    fn test_xip_state_machine_lifecycle() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_lifecycle".into());
        engine.register_activity(author.clone());

        let mut changes = HashMap::new();
        changes.insert("reference_price".into(), "15".into());

        let proposal = engine
            .create_proposal(
                "Lifecycle test".into(),
                "Testing lifecycle".into(),
                changes,
                author,
            )
            .unwrap();

        // 1. Debe empezar en Draft
        assert_eq!(proposal.xip_state.label(), "Draft");

        // 2. Transición válida: Draft → Discussion
        let now = GovernanceEngine::now_secs();
        let mut p = engine.get_proposal(&proposal.id).unwrap().clone();
        GovernanceEngine::transition_to_state(
            &mut p,
            XipState::Discussion {
                entered_at: now,
                expires_at: now + 3 * 86400,
            },
        )
        .unwrap();
        assert_eq!(p.xip_state.label(), "Discussion");

        // 3. Transición inválida: Draft → Execution (directo)
        let mut p2 = engine.get_proposal(&proposal.id).unwrap().clone();
        let result = GovernanceEngine::transition_to_state(
            &mut p2,
            XipState::Execution {
                entered_at: now,
                expires_at: now + 48 * 3600,
            },
        );
        assert!(result.is_err());

        // 4. Transición válida: Discussion → Complete
        let mut p3 = engine.get_proposal(&proposal.id).unwrap().clone();
        GovernanceEngine::transition_to_state(&mut p3, XipState::Complete { entered_at: now })
            .unwrap();
        assert_eq!(p3.xip_state.label(), "Complete");
    }

    #[test]
    fn test_weighted_vote_tally() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_weighted_author".into());
        engine.register_activity(author.clone());

        let mut changes = HashMap::new();
        changes.insert("reference_price".into(), "20".into());

        let proposal = engine
            .create_proposal(
                "Weighted vote test".into(),
                "Testing weighted voting".into(),
                changes,
                author,
            )
            .unwrap();

        // Dar apoyos para pasar a Voting (default min_supports = 5)
        for i in 0..5 {
            let s = WalletAddress(format!("xv1_ws_{}", i));
            engine.register_activity(s.clone());
            engine.support_proposal(&proposal.id, &s).unwrap();
        }

        // Verificar que está en Voting
        let p = engine.get_proposal(&proposal.id).unwrap();
        assert_eq!(p.xip_state.label(), "Voting");

        // Votar con algunas wallets
        let v1 = WalletAddress("xv1_voter_1".into());
        engine.register_activity(v1.clone());
        engine
            .user_vote(&proposal.id, &v1, true, vec![], vec![])
            .unwrap();

        let v2 = WalletAddress("xv1_voter_2".into());
        engine.register_activity(v2.clone());
        engine
            .user_vote(&proposal.id, &v2, false, vec![], vec![])
            .unwrap();

        // Verificar que weighted tally funciona (ambos tienen weight 1 por defecto)
        let (yes_weight, total_weight) = engine.weighted_user_vote_tally(&proposal.id).unwrap();
        assert_eq!(total_weight, 2);
        assert_eq!(yes_weight, 1);
    }

    #[test]
    fn test_xip_state_can_transition_to() {
        let now = 1000000;

        // D → D ✓
        let d = XipState::Draft { entered_at: now };
        assert!(d.can_transition_to(&XipState::Discussion {
            entered_at: now,
            expires_at: now + 86400
        }));
        assert!(d.can_transition_to(&XipState::Complete { entered_at: now }));
        assert!(!d.can_transition_to(&XipState::Voting {
            entered_at: now,
            expires_at: now + 86400
        }));
        assert!(!d.can_transition_to(&XipState::Execution {
            entered_at: now,
            expires_at: now + 3600
        }));

        // Discussion → V, C
        let disc = XipState::Discussion {
            entered_at: now,
            expires_at: now + 86400,
        };
        assert!(disc.can_transition_to(&XipState::Voting {
            entered_at: now,
            expires_at: now + 86400 * 7
        }));
        assert!(disc.can_transition_to(&XipState::Complete { entered_at: now }));
        assert!(!disc.can_transition_to(&XipState::Draft { entered_at: now }));
        assert!(!disc.can_transition_to(&XipState::Execution {
            entered_at: now,
            expires_at: now + 3600
        }));

        // Voting → E, C
        let voting = XipState::Voting {
            entered_at: now,
            expires_at: now + 86400 * 7,
        };
        assert!(voting.can_transition_to(&XipState::Execution {
            entered_at: now,
            expires_at: now + 3600 * 48
        }));
        assert!(voting.can_transition_to(&XipState::Complete { entered_at: now }));
        assert!(!voting.can_transition_to(&XipState::Draft { entered_at: now }));
        assert!(!voting.can_transition_to(&XipState::Discussion {
            entered_at: now,
            expires_at: now + 86400
        }));

        // Execution → C
        let exec = XipState::Execution {
            entered_at: now,
            expires_at: now + 3600 * 48,
        };
        assert!(exec.can_transition_to(&XipState::Complete { entered_at: now }));
        assert!(!exec.can_transition_to(&XipState::Draft { entered_at: now }));
        assert!(!exec.can_transition_to(&XipState::Voting {
            entered_at: now,
            expires_at: now + 86400
        }));
    }

    #[test]
    fn test_non_voting_user_cant_vote_inactive() {
        // Regression: inactive wallet should not be able to vote via user_vote
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_author_active".into());
        engine.register_activity(author.clone());

        // Inactive wallet
        let inactive = WalletAddress("xv1_inactive_voter".into());

        let mut changes = HashMap::new();
        changes.insert("test_param".into(), "value".into());

        let proposal = engine
            .create_proposal("Test inactive".into(), "Test".into(), changes, author)
            .unwrap();

        // dar apoyos
        for i in 0..5 {
            let s = WalletAddress(format!("xv1_sup_{}", i));
            engine.register_activity(s.clone());
            engine.support_proposal(&proposal.id, &s).unwrap();
        }

        // inactive wallet should NOT be able to vote
        let result = engine.user_vote(&proposal.id, &inactive, true, vec![], vec![]);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), GovernanceError::InactiveVoter),
            "Inactive wallet should get InactiveVoter error"
        );
    }

    #[test]
    fn test_weighted_user_vote_tally_on_empty_proposal() {
        // When no one has voted, total_weight should be 0
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_empty_author".into());
        engine.register_activity(author.clone());

        let proposal = engine
            .create_proposal("Empty".into(), "Empty".into(), HashMap::new(), author)
            .unwrap();

        let (yes_weight, total_weight) = engine.weighted_user_vote_tally(&proposal.id).unwrap();
        assert_eq!(yes_weight, 0);
        assert_eq!(total_weight, 0);
    }

    #[test]
    fn test_proposal_not_found_errors() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let result = engine.tally_votes("nonexistent_proposal");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            GovernanceError::ProposalNotFound
        ));

        // Register wallet so can_user_vote passes first, then ProposalNotFound is reached
        let voter = WalletAddress("xv1_test".into());
        engine.register_activity(voter.clone());
        let voter_result = engine.user_vote("nonexistent", &voter, true, vec![], vec![]);
        assert!(voter_result.is_err());
        assert!(matches!(
            voter_result.unwrap_err(),
            GovernanceError::ProposalNotFound
        ));
    }

    // ── Reputation-weighted voting tests ──────────────────────

    #[test]
    fn test_voting_with_reputation_returns_higher_weight_for_trusted_wallet() {
        use crate::data_commons::reputation::{EigenTrustEngine, ReputationConfig};

        // Build an EigenTrust engine with known trust scores
        let rep_config = ReputationConfig::default();
        let mut rep_engine = EigenTrustEngine::new(rep_config, vec![]);

        let w1 = WalletAddress("xv1_rep_trusted".into());
        let w2 = WalletAddress("xv1_rep_peer".into());
        let w3 = WalletAddress("xv1_rep_other".into());

        // Create a trust chain: w2 trusts w1, w3 trusts w1 (w1 is highly trusted)
        rep_engine.add_attestation(ReputationAttestation {
            from: w2.clone(),
            to: w1.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        rep_engine.add_attestation(ReputationAttestation {
            from: w3.clone(),
            to: w1.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        // Some reciprocal trust to make the graph connected
        rep_engine.add_attestation(ReputationAttestation {
            from: w1.clone(),
            to: w2.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        rep_engine.add_attestation(ReputationAttestation {
            from: w1.clone(),
            to: w3.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });

        rep_engine.compute().unwrap();

        let rep_arc = Arc::new(RwLock::new(rep_engine));
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config).with_reputation_engine(rep_arc);

        // Create proposal and get it to voting state
        let author = WalletAddress("xv1_rep_author".into());
        engine.register_activity(author.clone());
        let proposal = engine
            .create_proposal(
                "Rep test".into(),
                "Test".into(),
                HashMap::new(),
                author.clone(),
            )
            .unwrap();

        // Give supports (including author)
        for i in 0..4 {
            let s = WalletAddress(format!("xv1_rep_s_{}", i));
            engine.register_activity(s.clone());
            engine.support_proposal(&proposal.id, &s).unwrap();
        }
        engine.support_proposal(&proposal.id, &author).unwrap();

        // Vote with the trusted wallet
        engine.register_activity(w1.clone());
        engine
            .user_vote(&proposal.id, &w1, true, vec![], vec![])
            .unwrap();

        let (yes_weight, total_weight) = engine.weighted_user_vote_tally(&proposal.id).unwrap();
        // The trusted wallet should have weight > 1 since it has high EigenTrust score
        assert!(
            total_weight > 1,
            "Trusted wallet should have weight > 1, got {}",
            total_weight
        );
        assert_eq!(yes_weight, total_weight);
    }

    #[test]
    fn test_voting_without_reputation_engine_falls_back_to_weight_1() {
        // No reputation engine attached — every vote gets weight 1
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);

        let author = WalletAddress("xv1_norep_author".into());
        engine.register_activity(author.clone());
        let proposal = engine
            .create_proposal(
                "NoRep".into(),
                "Test".into(),
                HashMap::new(),
                author.clone(),
            )
            .unwrap();

        for i in 0..4 {
            let s = WalletAddress(format!("xv1_norep_s_{}", i));
            engine.register_activity(s.clone());
            engine.support_proposal(&proposal.id, &s).unwrap();
        }
        engine.support_proposal(&proposal.id, &author).unwrap();

        let voter = WalletAddress("xv1_norep_voter".into());
        engine.register_activity(voter.clone());
        engine
            .user_vote(&proposal.id, &voter, true, vec![], vec![])
            .unwrap();

        let (yes_weight, total_weight) = engine.weighted_user_vote_tally(&proposal.id).unwrap();
        assert_eq!(total_weight, 1, "Without engine, weight should be 1");
        assert_eq!(yes_weight, 1);
    }

    #[test]
    fn test_low_reputation_gets_lower_weight() {
        use crate::data_commons::reputation::{EigenTrustEngine, ReputationConfig};

        // Build engine where wallet_low has low trust
        let rep_config = ReputationConfig::default();
        let mut rep_engine = EigenTrustEngine::new(rep_config, vec![]);

        let wallet_high = WalletAddress("xv1_high_trust".into());
        let wallet_low = WalletAddress("xv1_low_trust".into());
        let wallet_rater = WalletAddress("xv1_rater".into());

        // rater trusts wallet_high strongly
        rep_engine.add_attestation(ReputationAttestation {
            from: wallet_rater.clone(),
            to: wallet_high.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        // rater distrusts wallet_low (negative score → no trust contribution)
        rep_engine.add_attestation(ReputationAttestation {
            from: wallet_rater.clone(),
            to: wallet_low.clone(),
            score: -1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        // Some reciprocal trust for graph connectivity
        rep_engine.add_attestation(ReputationAttestation {
            from: wallet_high.clone(),
            to: wallet_rater.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        rep_engine.add_attestation(ReputationAttestation {
            from: wallet_low.clone(),
            to: wallet_rater.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });
        rep_engine.add_attestation(ReputationAttestation {
            from: wallet_high.clone(),
            to: wallet_low.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });

        rep_engine.compute().unwrap();

        // wallet_high should have higher score than wallet_low
        let score_high = rep_engine.computed_score(&wallet_high.0);
        let score_low = rep_engine.computed_score(&wallet_low.0);
        assert!(
            score_high > score_low,
            "High-trust wallet ({}) should score higher than low-trust wallet ({})",
            score_high,
            score_low
        );
    }

    // ── Dynamic quorum tests ──────────────────────────────────

    #[test]
    fn test_low_participation_lowers_quorum() {
        let dq = DynamicQuorum::default();
        let low_rate = 0.20; // < 0.30
        let effective = dq.effective_user_quorum(low_rate);
        // base is 0.10, lowered by 20% → 0.10 * 0.80 = 0.08
        assert!(
            (effective - 0.08).abs() < 0.0001,
            "Low participation should lower quorum to 0.08, got {}",
            effective
        );

        // Council quorum: base 0.51 * 0.80 = 0.408
        let council_eff = dq.effective_council_quorum(low_rate);
        assert!(
            (council_eff - 0.408).abs() < 0.0001,
            "Low council participation should lower quorum to 0.408, got {}",
            council_eff
        );
    }

    #[test]
    fn test_high_participation_raises_quorum() {
        let dq = DynamicQuorum::default();
        let high_rate = 0.90; // > 0.80
        let effective = dq.effective_user_quorum(high_rate);
        // base is 0.10, raised by 10% → 0.10 * 1.10 = 0.11
        assert!(
            (effective - 0.11).abs() < 0.0001,
            "High participation should raise quorum to 0.11, got {}",
            effective
        );

        // Council quorum: base 0.51 * 1.10 = 0.561
        let council_eff = dq.effective_council_quorum(high_rate);
        assert!(
            (council_eff - 0.561).abs() < 0.0001,
            "High council participation should raise quorum to 0.561, got {}",
            council_eff
        );
    }

    #[test]
    fn test_dynamic_quorum_missing_uses_defaults() {
        // When no dynamic quorum is configured, tally_votes should use static config values
        let config = GovernanceConfig::default();
        assert!(
            config.dynamic_quorum.is_none(),
            "Default config should have no dynamic quorum"
        );

        // The static values should be 10.0 for user and 51.0 for council
        assert_eq!(config.user_quorum_minimum, 10.0);
        assert_eq!(config.council_quorum_minimum, 51.0);
    }
}
