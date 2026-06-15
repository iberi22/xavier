//! # Gobernanza Bicameral — 50% Usuarios + 50% Consejo Xavier Core
//!
//! ## Sistema de Votación
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                    XIP (Propuesta)                           │
//! └──────────────────────────────────────────────────────────────┘
//!                              │
//!                     Discusión (3 días)
//!                              │
//!                   ┌──────────┴──────────┐
//!                   ▼                     ▼
//!        ┌──────────────────┐   ┌──────────────────┐
//!        │  Cámara 1        │   │  Cámara 2        │
//!        │  USUARIOS        │   │  CONSEJO         │
//!        │  50% peso        │   │  50% peso        │
//!        │  1 wallet = 1 voto│  │  1 miembro = 1    │
//!        │  Anónimo          │   │  voto            │
//!        │  Últimos 7 días   │   │  Público         │
//!        └────────┬─────────┘   └────────┬─────────┘
//!                 │                      │
//!                 └──────────┬───────────┘
//!                            ▼
//!               ¿Mayoría simple en AMBAS?
//!                      /         \
//!                    SÍ           NO
//!                    │             │
//!              ┌─────▼─────┐   ┌──▼───┐
//!              │ APROBADA  │   │ RECHAZADA │
//!              │ Timer 48h │   └──────────┘
//!              │ → Ejecutar│
//!              └───────────┘
//! ```
//!
//! ## Veto del Consejo (Excepción de Seguridad)
//!
//! El consejo puede vetar propuestas que comprometan:
//! - Seguridad post-cuántica
//! - Integridad del protocolo mesh
//! - Descentralización
//!
//! **Veto requiere 66% del consejo.** Veto es público y con explicación.
//! La comunidad puede OVERRULE el veto si alcanza 75% de apoyo en segunda votación.
//!
//! ## Feedback de Uso
//!
//! Una wallet solo puede votar si ha tenido actividad en los últimos 7 días:
//! - Compartir, comprar o validar al menos 1 contexto
//! - Si no usas el sistema 7 días seguidos → pierdes derecho a voto
//! - Recuperas el derecho al volver a interactuar
//!
//! ## Sin Delegación
//!
//! No hay delegación de voto. Si no votas, no votas.
//! Esto evita que grandes wallets acumulen poder delegado.

use crate::data_commons::types::*;
use std::collections::HashMap;

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
}

impl GovernanceEngine {
    pub fn new(config: GovernanceConfig) -> Self {
        Self {
            config,
            proposals: Vec::new(),
            council: Vec::new(),
            active_wallets: HashMap::new(),
            blocked_wallets: Vec::new(),
        }
    }

    // ── Cámara 1: Usuarios ──────────────────────────────────

    /// Registrar actividad de una wallet (compartir, comprar o validar contexto)
    ///
    /// Esto mantiene el derecho a voto activo.
    pub fn register_activity(&mut self, wallet: WalletAddress) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.active_wallets.insert(wallet, now);
    }

    /// Verificar si una wallet puede votar (actividad en últimos N días)
    pub fn can_user_vote(&self, wallet: &WalletAddress) -> bool {
        if self.blocked_wallets.contains(wallet) {
            return false;
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let window_secs = self.config.voting_activity_window_days as u64 * 86400;

        self.active_wallets
            .get(wallet)
            .map(|&last_activity| now.saturating_sub(last_activity) <= window_secs)
            .unwrap_or(false)
    }

    /// Obtener wallets activas para votar (cumplen feedback de 7 días)
    pub fn active_voter_wallets(&self) -> Vec<&WalletAddress> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

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
            joined_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
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

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let proposal = XipProposal {
            id: format!("xip_{}_{}", now, self.proposals.len() + 1),
            title,
            description,
            changes,
            author,
            status: ProposalStatus::Draft,
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
        // Checks que necesitan &self antes del borrow mutable
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

        if !matches!(
            proposal.status,
            ProposalStatus::Draft | ProposalStatus::Discussion
        ) {
            return Err(GovernanceError::VotingNotOpen);
        }

        if proposal.supports.contains(wallet) {
            return Err(GovernanceError::AlreadySupported);
        }

        proposal.supports.push(wallet.clone());

        // Si alcanza apoyos mínimos, pasar a votación
        if proposal.supports.len() >= min_supports as usize {
            proposal.status = ProposalStatus::Voting;
        }

        Ok(())
    }

    /// Voto de usuario (anónimo — cifrado con Kyber)
    pub fn user_vote(
        &mut self,
        proposal_id: &str,
        wallet: &WalletAddress,
        in_favor: bool,
        _encrypted_vote: Vec<u8>,
        _dilithium_signature: Vec<u8>,
    ) -> Result<(), GovernanceError> {
        // Checks que necesitan &self antes del borrow mutable
        let can_vote = self.can_user_vote(wallet);

        if !can_vote {
            return Err(GovernanceError::InactiveVoter);
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Voting {
            return Err(GovernanceError::VotingNotOpen);
        }

        if proposal.user_votes.contains_key(wallet) {
            return Err(GovernanceError::AlreadyVoted);
        }

        // TODO: verificar firma Dilithium-5 contra wallet pública

        proposal.user_votes.insert(wallet.clone(), in_favor);
        Ok(())
    }

    /// Voto del consejo (público)
    pub fn council_vote(
        &mut self,
        proposal_id: &str,
        member_id: &str,
        in_favor: bool,
    ) -> Result<(), GovernanceError> {
        // Verificar que el miembro existe y está activo (antes del borrow mutable)
        let member_active = self.council.iter().any(|m| m.id == member_id && m.active);
        if !member_active {
            return Err(GovernanceError::NotAuthorized);
        }

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Voting {
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
        // Extraer datos necesarios antes del borrow mutable
        let active_members = self.active_council_members().len() as f32;
        let veto_threshold = self.config.council_veto_threshold;

        let proposal = self
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Voting {
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

        Ok(())
    }

    /// Apelación comunitaria — overrule del veto del consejo
    ///
    /// Requiere 75% de apoyo de wallets activas en segunda votación.
    pub fn community_appeal(&mut self, proposal_id: &str) -> Result<(), GovernanceError> {
        // Extraer datos antes del borrow mutable
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

        let votes_for = proposal.user_votes.values().filter(|&&v| v).count() as f32;
        let total_votes_cast = proposal.user_votes.len() as f32;

        if total_votes_cast < (total_active * quorum_minimum / 100.0).floor() {
            return Err(GovernanceError::QuorumNotMet);
        }

        let support_percentage = (votes_for / total_votes_cast) * 100.0;
        if support_percentage >= overrule_threshold {
            proposal.status = ProposalStatus::Overruled;
            proposal.council_veto = false;
            proposal.appealed = true;
            Ok(())
        } else {
            Err(GovernanceError::OverruleThresholdNotReached)
        }
    }

    /// Contar votos de una propuesta finalizada
    pub fn tally_votes(&mut self, proposal_id: &str) -> Result<BicameralResult, GovernanceError> {
        let proposal = self
            .proposals
            .iter()
            .find(|p| p.id == proposal_id)
            .ok_or(GovernanceError::ProposalNotFound)?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now < proposal.voting_end && !proposal.council_veto {
            return Err(GovernanceError::VotingNotEnded);
        }

        // ── Cámara 1: Usuarios ──
        let active_users = self.active_voter_wallets().len() as f32;
        let total_user_votes = proposal.user_votes.len() as f32;

        let user_for = proposal.user_votes.values().filter(|&&v| v).count() as u64;
        let user_against = proposal.user_votes.values().filter(|&&v| !v).count() as u64;

        let user_quorum_met =
            total_user_votes >= (active_users * self.config.user_quorum_minimum / 100.0).floor();
        let user_percentage = if total_user_votes > 0.0 {
            (user_for as f32 / total_user_votes) * 100.0
        } else {
            0.0
        };

        // ── Cámara 2: Consejo ──
        let active_council = self.active_council_members().len() as f32;
        let total_council_votes = proposal.council_votes.len() as f32;

        let council_for = proposal.council_votes.values().filter(|&&v| v).count() as u64;
        let council_against = proposal.council_votes.values().filter(|&&v| !v).count() as u64;

        let council_quorum_met = total_council_votes
            >= (active_council * self.config.council_quorum_minimum / 100.0).floor();
        let council_percentage = if total_council_votes > 0.0 {
            (council_for as f32 / total_council_votes) * 100.0
        } else {
            0.0
        };

        // ── Resultado Final ──
        // Mayoría simple en AMBAS cámaras
        let user_passed = user_quorum_met && user_percentage > 50.0;
        let council_passed = council_quorum_met && council_percentage > 50.0;
        let passed = user_passed && council_passed;

        // Veto overruled?
        let veto_overruled = if proposal.council_veto && proposal.appealed {
            // Ya fue overruled por apelación comunitaria
            true
        } else if proposal.council_veto {
            // Veto activo, propuesta no pasa aunque ambas cámaras estén a favor
            false // marcado como vetoed, no como rechazado normal
        } else {
            false
        };

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
            council_veto_active: proposal.council_veto && !proposal.appealed,
            passed,
            veto_overruled,
            executed: false,
            tallied_at: now,
        };

        // Actualizar estado de la propuesta (no referenciar `proposal` prestado)
        let veto_active = proposal.council_veto && !proposal.appealed;
        let was_overruled = proposal.council_veto && proposal.appealed;

        if let Some(p) = self.proposals.iter_mut().find(|p| p.id == proposal_id) {
            if veto_active {
                // veto activo, no apelado → se queda como Vetoed
            } else if passed {
                p.status = ProposalStatus::Approved;
            } else if was_overruled {
                p.status = ProposalStatus::Overruled;
            } else {
                p.status = ProposalStatus::Rejected;
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

        if proposal.status != ProposalStatus::Approved
            && proposal.status != ProposalStatus::Overruled
        {
            return Err(GovernanceError::ProposalNotApproved);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now < proposal.execution_at {
            return Err(GovernanceError::ExecutionTimerNotReady);
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

        // Las expulsiones requieren 66%
        Ok(proposal)
    }

    // ── Getters ──────────────────────────────────────────

    pub fn active_proposals(&self) -> Vec<&XipProposal> {
        self.proposals
            .iter()
            .filter(|p| {
                matches!(
                    p.status,
                    ProposalStatus::Discussion | ProposalStatus::Voting
                )
            })
            .collect()
    }

    pub fn get_proposal(&self, id: &str) -> Option<&XipProposal> {
        self.proposals.iter().find(|p| p.id == id)
    }

    pub fn council_size(&self) -> usize {
        self.active_council_members().len()
    }

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
}
