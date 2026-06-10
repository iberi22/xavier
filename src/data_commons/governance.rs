//! # Gobernanza — 100% Democrática, 100% Anónima
//!
//! ## Sistema de Votación
//!
//! - **1 wallet = 1 voto.** Sin importar saldo de $XAV
//! - **Voto anónimo:** Cifrado con Kyber, revelado solo al contar
//! - **Quórum:** 10% de wallets activas
//! - **Período:** 7 días
//! - **Mayoría simple:** >50% gana
//! - **Timer ejecución:** 48h post-aprobación
//!
//! ## Sin Delegación
//!
//! No hay delegación de voto. Si no votas, no votas.
//! Esto evita que grandes wallets acumulen poder delegado.
//!
//! ## Parámetros Gobernables
//!
//! Todos los parámetros del sistema son modificables por voto:
//! - PrecioReferencia, splits, rate limits, burn rate
//! - Multiplicadores de categoría
//! - Período de votación, quórum
//! - Pre-trusted seeds, expulsión por collusion

use crate::data_commons::types::*;
use std::collections::HashMap;

/// Configuración de gobernanza
#[derive(Debug, Clone)]
pub struct GovernanceConfig {
    /// Período de discusión en días (default: 3)
    pub discussion_period_days: u32,
    /// Período de votación en días (default: 7)
    pub voting_period_days: u32,
    /// Timer de ejecución en horas (default: 48)
    pub execution_timer_hours: u32,
    /// Quórum mínimo en % (default: 10)
    pub quorum_minimum: f32,
    /// Apoyos mínimos para pasar a votación (default: 5)
    pub min_supports: u32,
    /// Votos requeridos para expulsión por collusion (default: 66%)
    pub expulsion_threshold: f32,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            discussion_period_days: 3,
            voting_period_days: 7,
            execution_timer_hours: 48,
            quorum_minimum: 10.0,
            min_supports: 5,
            expulsion_threshold: 66.0,
        }
    }
}

/// Motor de gobernanza
pub struct GovernanceEngine {
    config: GovernanceConfig,
    /// Propuestas activas
    proposals: Vec<XipProposal>,
    /// Wallets activas (que han votado en el último mes)
    active_wallets: Vec<WalletAddress>,
    /// Wallets bloqueadas por collusion
    blocked_wallets: Vec<WalletAddress>,
}

impl GovernanceEngine {
    pub fn new(config: GovernanceConfig) -> Self {
        Self {
            config,
            proposals: Vec::new(),
            active_wallets: Vec::new(),
            blocked_wallets: Vec::new(),
        }
    }

    /// Crear una nueva propuesta (XIP)
    pub fn create_proposal(
        &mut self,
        title: String,
        description: String,
        changes: HashMap<String, String>,
        author: WalletAddress,
    ) -> Result<XipProposal, GovernanceError> {
        todo!("Feature 6.3 — Create XIP proposal")
    }

    /// Apoyar una propuesta (para pasar a votación)
    pub fn support_proposal(&mut self, proposal_id: &str, wallet: &WalletAddress) -> Result<(), GovernanceError> {
        todo!("Feature 6.3 — Support proposal")
    }

    /// Emitir voto (anónimo — cifrado con Kyber)
    pub fn vote(
        &mut self,
        proposal_id: &str,
        wallet: &WalletAddress,
        in_favor: bool,
        encrypted_vote: Vec<u8>,
        signature: Vec<u8>,
    ) -> Result<(), GovernanceError> {
        todo!("Feature 6.1 — Cast vote")
    }

    /// Contar votos de una propuesta finalizada
    pub fn tally_votes(&mut self, proposal_id: &str) -> Result<ProposalStatus, GovernanceError> {
        todo!("Feature 6.1 — Tally votes")
    }

    /// Ejecutar una propuesta aprobada
    ///
    /// Aplica los cambios a los parámetros del sistema.
    /// Timer de 48h entre aprobación y ejecución.
    pub fn execute_proposal(&mut self, proposal_id: &str, system_params: &mut SystemParams) -> Result<(), GovernanceError> {
        todo!("Feature 6.2 — Execute approved proposal")
    }

    /// Proponer expulsión de wallet por collusion comprobada
    /// Requiere 66% de votos
    pub fn propose_expulsion(
        &mut self,
        target: WalletAddress,
        evidence: String,
        author: WalletAddress,
    ) -> Result<XipProposal, GovernanceError> {
        todo!("Feature 6.2 — Propose expulsion")
    }

    /// Listar propuestas activas
    pub fn active_proposals(&self) -> Vec<&XipProposal> {
        self.proposals.iter().filter(|p| {
            matches!(p.status, ProposalStatus::Discussion | ProposalStatus::Voting)
        }).collect()
    }

    /// Obtener propuesta por ID
    pub fn get_proposal(&self, id: &str) -> Option<&XipProposal> {
        self.proposals.iter().find(|p| p.id == id)
    }

    /// Verificar si una wallet puede votar
    pub fn can_vote(&self, wallet: &WalletAddress) -> bool {
        !self.blocked_wallets.contains(wallet)
    }

    /// Calcular wallets activas (las que votaron en ≥1 mes)
    pub fn refresh_active_wallets(&mut self) -> usize {
        let one_month_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - 30 * 86400;

        let active: Vec<WalletAddress> = self.proposals
            .iter()
            .flat_map(|p| p.votes.keys().cloned())
            .filter(|w| {
                // Wallet votó en alguna propuesta reciente
                self.proposals.iter().any(|p| {
                    p.votes.contains_key(w) && p.voting_end > one_month_ago
                })
            })
            .collect();

        let count = active.len();
        self.active_wallets = active;
        count
    }
}

#[derive(Debug)]
pub enum GovernanceError {
    NotAuthorized,
    ProposalNotFound,
    AlreadyVoted,
    VotingNotOpen,
    InsufficientSupports,
    QuorumNotMet,
    ExecutionTimerNotReady,
    WalletBlocked,
    InvalidProposal,
    WalletAlreadyBlocked,
}

impl std::fmt::Display for GovernanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized => write!(f, "No autorizado para esta acción"),
            Self::ProposalNotFound => write!(f, "Propuesta no encontrada"),
            Self::AlreadyVoted => write!(f, "Ya votaste en esta propuesta"),
            Self::VotingNotOpen => write!(f, "La votación no está abierta"),
            Self::InsufficientSupports => write!(f, "Se necesitan más apoyos para pasar a votación"),
            Self::QuorumNotMet => write!(f, "No se alcanzó el quórum mínimo"),
            Self::ExecutionTimerNotReady => write!(f, "El timer de ejecución de 48h no ha expirado"),
            Self::WalletBlocked => write!(f, "Esta wallet está bloqueada por collusion"),
            Self::InvalidProposal => write!(f, "Propuesta inválida"),
            Self::WalletAlreadyBlocked => write!(f, "La wallet ya está bloqueada"),
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
        
        let mut changes = HashMap::new();
        changes.insert("reference_price".into(), "10".into());
        
        let author = WalletAddress("xv1_test_author".into());
        let proposal = engine.create_proposal(
            "Aumentar precio referencial".into(),
            "Propongo subir el precio referencial de 5 a 10 $XAV".into(),
            changes,
            author,
        );
        
        assert!(proposal.is_ok());
        assert_eq!(engine.proposals.len(), 1);
    }

    #[test]
    fn test_one_wallet_one_vote() {
        let config = GovernanceConfig::default();
        let mut engine = GovernanceEngine::new(config);
        
        let wallet = WalletAddress("xv1_voter".into());
        assert!(engine.can_vote(&wallet));
    }
}
