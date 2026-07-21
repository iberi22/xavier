//! # Tipos Centrales de Data Commons
//!
//! Define los tipos de datos compartidos en toda la red Data Commons.
//! Estos tipos son el contrato entre nodos — cualquier cambio requiere
//! una propuesta XIP aprobada por gobernanza.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Wallet
// ---------------------------------------------------------------------------

/// Dirección pública de wallet $XAV
///
/// Formato: `xv1_` + hash(public_key ML-DSA-87) en bech32
/// Ejemplo: `xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WalletAddress(pub String);

impl WalletAddress {
    /// Is valid.
    pub fn is_valid(&self) -> bool {
        self.0.starts_with("xv1_") && self.0.len() == 65
    }
}

/// Estado completo de una wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    /// Dirección pública de la wallet
    pub address: WalletAddress,
    /// Clave pública ML-DSA-87 (Dilithium-5) serializada
    pub dilithium_public_key: Vec<u8>,
    /// Clave pública ML-KEM-1024 (Kyber-1024) serializada
    pub kyber_public_key: Vec<u8>,
    /// Nodos registrados bajo esta wallet
    pub nodes: Vec<NodeBinding>,
    /// Saldo actual de $XAV
    pub balance: u64,
    /// Trust score EigenTrust (-1000 a +1000, scaled)
    pub trust_score: i64,
    /// Contribution score (0-1000)
    pub contribution_score: u64,
    /// Fecha de creación (Unix timestamp)
    pub created_at: u64,
    /// Wallet usa TPM hardware?
    pub has_tpm: bool,
}

/// Binding de un nodo a una wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBinding {
    /// NodeID del nodo (ya existente: xv1- + hash Ed25519)
    pub node_id: String,
    /// Firma Dilithium-5 de (NodeID + WalletAddress) por parte del wallet
    pub signature: Vec<u8>,
    /// Fecha de registro
    pub registered_at: u64,
    /// Último heartbeat del nodo
    pub last_heartbeat_at: u64,
    /// Estado del binding
    pub status: BindingStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BindingStatus {
    Active,
    Revoked,
    Suspended,
}

// ---------------------------------------------------------------------------
// Datos Técnicos (Contextos)
// ---------------------------------------------------------------------------

/// Categoría de dato técnico compartible
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataCategory {
    /// Error crítico — crash, data loss, pánico
    CriticalError,
    /// Error funcional — feature no funciona como esperado
    FunctionalError,
    /// Benchmark de rendimiento — CPU, RAM, disco, red
    Benchmark,
    /// Log normal — información general
    NormalLog,
    /// Telemetría básica — versión, uptime, módulos activos
    BasicTelemetry,
    /// Anomalía de comportamiento — algo inusual pero no error
    Anomaly,
}

/// Metadata pública de un contexto (visible sin comprar)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextOffer {
    /// Hash SHA-256 del contenido (ID único)
    pub context_hash: String,
    /// Categoría del dato
    pub category: DataCategory,
    /// Módulo de Xavier afectado
    pub module: String,
    /// Rareza: qué % de nodos reportaron esto
    pub rarity: f32,
    /// Trust score del vendedor al momento de publicar
    pub seller_trust: i64,
    /// Precio en $XAV
    pub price: u64,
    /// Timestamp de publicación
    pub published_at: u64,
    /// Dirección del vendedor
    pub seller_address: WalletAddress,
}

/// Contexto completo (compartido internamente — cifrado en tránsito)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    /// Metadata pública
    pub offer: ContextOffer,
    /// Contenido cifrado con Kyber-1024 del comprador
    pub encrypted_content: Vec<u8>,
    /// Firma Dilithium-5 del vendedor sobre (context_hash + encrypted_content)
    pub signature: Vec<u8>,
}

/// Feedback del comprador sobre un contexto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feedback {
    pub context_hash: String,
    pub buyer_address: WalletAddress,
    /// +1 útil, -1 basura, 0 neutral
    pub score: i8,
    pub timestamp: u64,
    /// Firma Dilithium-5 del comprador
    pub signature: Vec<u8>,
}

// ---------------------------------------------------------------------------
// MINTER & Tokenomics
// ---------------------------------------------------------------------------

/// Evento de minteo (emisión de $XAV)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinterEvent {
    /// Hash de la transacción
    pub tx_hash: String,
    /// Wallet receptora
    pub beneficiary: WalletAddress,
    /// Cantidad de $XAV minteados
    pub amount: u64,
    /// Desglose de la recompensa
    pub breakdown: RewardBreakdown,
    /// Timestamp
    pub minted_at: u64,
    /// Firma Dilithium-5 del minter del sistema
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardBreakdown {
    /// Por compartir contexto (40%)
    pub node_reward: u64,
    /// Para el wallet (40%)
    pub wallet_reward: u64,
    /// Reserva de red (20%)
    pub network_reserve: u64,
    /// Factores que influyeron en el cálculo
    pub factors: RewardFactors,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardFactors {
    pub base_price: u64,
    pub rarity_multiplier: f32,
    pub trust_multiplier: f32,
    pub category_multiplier: f32,
    pub final_amount: u64,
}

/// Evento de quema de $XAV
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnEvent {
    pub tx_hash: String,
    pub burner: WalletAddress,
    pub amount: u64,
    pub context_hash: String,
    pub burned_at: u64,
    pub signature: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Reputación
// ---------------------------------------------------------------------------

/// Estado EigenTrust de un wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EigenTrustState {
    pub wallet: WalletAddress,
    /// Trust score global (-1000 a +1000)
    pub global_trust: i64,
    /// Vectores de confianza local (quién confía en quién)
    pub local_trust: HashMap<WalletAddress, i64>,
    /// Pre-trusted? (seed node de Xavier Core)
    pub is_pre_trusted: bool,
    /// Última actualización
    pub last_updated: u64,
}

/// Resultado de una iteración EigenTrust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EigenTrustResult {
    /// Mapa de wallet → trust score
    pub scores: HashMap<WalletAddress, f64>,
    /// Número de iteraciones hasta convergencia
    pub iterations: u32,
    /// Diferencia final (debe ser < 0.001)
    pub convergence_diff: f64,
    /// Timestamp
    pub computed_at: u64,
}

/// Atestación de reputación entre wallets
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationAttestation {
    pub from: WalletAddress,
    pub to: WalletAddress,
    /// +1 (endorse), -1 (report), 0 (neutral)
    pub score: i8,
    /// Contexto opcional que motivó esta atestación
    pub context_hash: Option<String>,
    pub timestamp: u64,
    pub signature: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Gobernanza
// ---------------------------------------------------------------------------

/// Estados del ciclo de vida de un XIP (Draft → Discussion → Voting → Execution → Complete)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum XipState {
    /// Borrador inicial
    Draft { entered_at: u64 },
    /// Discusión abierta (3 días)
    Discussion { entered_at: u64, expires_at: u64 },
    /// Votación activa (7 días)
    Voting { entered_at: u64, expires_at: u64 },
    /// Propuesta aprobada, en espera de ejecución (48h)
    Execution { entered_at: u64, expires_at: u64 },
    /// Completado (ejecutado o finalizado)
    Complete { entered_at: u64 },
}

impl XipState {
    /// Validar si una transición de estado es legal
    pub fn can_transition_to(&self, new_state: &XipState) -> bool {
        matches!(
            (self, new_state),
            (XipState::Draft { .. }, XipState::Discussion { .. })
                | (XipState::Draft { .. }, XipState::Complete { .. })
                | (XipState::Discussion { .. }, XipState::Voting { .. })
                | (XipState::Discussion { .. }, XipState::Complete { .. })
                | (XipState::Voting { .. }, XipState::Execution { .. })
                | (XipState::Voting { .. }, XipState::Complete { .. })
                | (XipState::Execution { .. }, XipState::Complete { .. })
        )
    }

    /// Label.
    pub fn label(&self) -> &str {
        match self {
            XipState::Draft { .. } => "Draft",
            XipState::Discussion { .. } => "Discussion",
            XipState::Voting { .. } => "Voting",
            XipState::Execution { .. } => "Execution",
            XipState::Complete { .. } => "Complete",
        }
    }
}

/// Propuesta de mejora (XIP)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XipProposal {
    /// ID único de la propuesta
    pub id: String,
    /// Título corto
    pub title: String,
    /// Descripción detallada
    pub description: String,
    /// Parámetros a cambiar
    pub changes: HashMap<String, String>,
    /// Wallet creadora
    pub author: WalletAddress,
    /// Estado de la propuesta (legacy — kept for backward compat)
    pub status: ProposalStatus,
    /// Nuevo ciclo de vida XIP (en paralelo con status legacy)
    pub xip_state: XipState,
    /// Timestamps
    pub created_at: u64,
    pub discussion_end: u64,
    pub voting_end: u64,
    pub execution_at: u64,
    /// Votos de usuarios: wallet → vote (true = a favor)
    pub user_votes: HashMap<WalletAddress, bool>,
    /// Votos ponderados por reputación: wallet → WeightedVote
    pub weighted_user_votes: HashMap<WalletAddress, WeightedVote>,
    /// Votos del consejo: member_id → vote
    pub council_votes: HashMap<String, bool>,
    /// Apoyos (wallets que apoyan la propuesta para pasar a votación)
    pub supports: Vec<WalletAddress>,
    /// Veto activo del consejo?
    pub council_veto: bool,
    /// Razón del veto (si aplica)
    pub veto_reason: Option<String>,
    /// La propuesta fue apelada por la comunidad?
    pub appealed: bool,
}

/// Voto ponderado por reputación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedVote {
    pub wallet_id: WalletAddress,
    /// Peso del voto (trust score normalizado)
    pub weight: u64,
    /// A favor (true) o en contra (false)
    pub approve: bool,
    /// Timestamp del voto
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    Draft,
    Discussion,
    Voting,
    Approved,
    Rejected,
    Vetoed,
    Overruled, // La comunidad overruleó el veto del consejo
    Executed,
    Expired,
}

/// Miembro del consejo Xavier Core
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouncilMember {
    /// ID único del miembro
    pub id: String,
    /// Wallet asociada
    pub wallet: WalletAddress,
    /// Rol dentro del core
    pub role: CouncilRole,
    /// Fecha de ingreso al consejo
    pub joined_at: u64,
    /// Activo?
    pub active: bool,
    /// Expertise área (seguridad, arquitectura, skills...)
    pub expertise: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CouncilRole {
    CoreMaintainer,
    SkillContributor,
    SecurityAuditor,
    Architect,
    CommunityRepresentative,
}

/// Resultado de una votación bicameral
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BicameralResult {
    /// ID de la propuesta
    pub proposal_id: String,
    // Cámara 1: Usuarios
    pub user_votes_for: u64,
    pub user_votes_against: u64,
    pub user_abstain: u64,
    pub user_quorum_met: bool,
    pub user_percentage_for: f32,
    pub user_active_wallets: u64,
    // Cámara 2: Consejo
    pub council_votes_for: u64,
    pub council_votes_against: u64,
    pub council_total: u64,
    pub council_percentage_for: f32,
    pub council_veto_active: bool,
    // Resultado final
    pub passed: bool,
    pub veto_overruled: bool,
    pub executed: bool,
    pub tallied_at: u64,
}

// ---------------------------------------------------------------------------
// Parámetros del Sistema (gobernables)
// ---------------------------------------------------------------------------

/// Parámetros del sistema modificables por gobernanza
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemParams {
    /// Precio de referencia para contextos (default: 5)
    pub reference_price: u64,
    /// Multiplicadores por categoría
    pub category_multipliers: HashMap<String, f32>,
    /// Split de recompensas: [nodo%, wallet%, red%]
    pub reward_split: [u8; 3],
    /// Rate limit diario para wallets con trust < 0.3
    pub rate_limit_low_trust: u32,
    /// Burn rate (default: 80%)
    pub burn_rate: u8,
    /// Período de votación en días (default: 7)
    pub voting_period_days: u32,
    /// Quórum mínimo (% de wallets activas)
    pub quorum_minimum: f32,
    /// Precio mínimo de contexto (default: 1)
    pub min_price: u64,
    /// Precio máximo de contexto (default: 10,000)
    pub max_price: u64,
}

impl Default for SystemParams {
    fn default() -> Self {
        Self {
            reference_price: 5,
            category_multipliers: [
                ("CriticalError".into(), 3.0),
                ("FunctionalError".into(), 2.0),
                ("Benchmark".into(), 1.5),
                ("NormalLog".into(), 1.0),
                ("BasicTelemetry".into(), 0.5),
                ("Anomaly".into(), 2.5),
            ]
            .into(),
            reward_split: [40, 40, 20],
            rate_limit_low_trust: 10,
            burn_rate: 80,
            voting_period_days: 7,
            quorum_minimum: 10.0,
            min_price: 1,
            max_price: 10_000,
        }
    }
}
