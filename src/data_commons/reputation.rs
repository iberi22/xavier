//! # Reputación Descentralizada — EigenTrust Adaptado
//!
//! ## Algoritmo
//!
//! EigenTrust (Stanford 2003) adaptado para Xavier Data Commons:
//!
//! 1. **Señales locales:** Cada nodo registra interacciones con peers
//!    - +1: contexto útil (el fix funcionó)
//!    - -1: contexto basura (no aplicaba o era falso)
//!    - 0: neutral (no hay feedback o no aplica)
//!
//! 2. **Normalización:** c_ij = max(s_ij, 0) / Σ max(s_ij, 0)
//!
//! 3. **Power iteration:** t^(k+1) = (1-a) × C^T × t^(k) + a × p
//!    - a = 0.15 (teletransporte — probabilidad de ir a pre-trusted)
//!    - p = vector de pre-trusted peers
//!    - Convergencia: ||t^(k+1) - t^(k)|| < 0.001
//!
//! 4. **Distrust adjustment:** Ajuste negativo one-shot post-iteración
//!
//! 5. **Reputación híbrida:** 0.7 × EigenTrust + 0.3 × ContributionScore
//!
//! ## Anti-Manipulación
//!
//! - **Sybil:** Proof of Liveliness + rate limiting + trust threshold
//! - **Collusion:** EigenTrust detecta subgrafos densos de co-validación
//! - **Self-dealing:** Misma seed → transacción rechazada
//! - **Replay:** Hash SHA-256 único por contexto

use crate::data_commons::types::*;

/// Configuración del sistema de reputación
#[derive(Debug, Clone)]
pub struct ReputationConfig {
    /// Factor de teletransporte (default: 0.15)
    pub teleport_factor: f64,
    /// Threshold de convergencia (default: 0.001)
    pub convergence_threshold: f64,
    /// Máximo de iteraciones (default: 100)
    pub max_iterations: u32,
    /// Peso de EigenTrust en reputación final (default: 0.7)
    pub eigentrust_weight: f64,
    /// Peso de ContributionScore (default: 0.3)
    pub contribution_weight: f64,
}

impl Default for ReputationConfig {
    fn default() -> Self {
        Self {
            teleport_factor: 0.15,
            convergence_threshold: 0.001,
            max_iterations: 100,
            eigentrust_weight: 0.7,
            contribution_weight: 0.3,
        }
    }
}

/// Motor de reputación EigenTrust
pub struct EigenTrustEngine {
    config: ReputationConfig,
    /// Wallets pre-trusted (seed nodes de Xavier Core)
    #[allow(dead_code)]
    pre_trusted: Vec<WalletAddress>,
    /// Atestaciones de reputación recolectadas
    attestations: Vec<ReputationAttestation>,
    /// Resultado del último cómputo
    last_result: Option<EigenTrustResult>,
}

impl EigenTrustEngine {
    /// Crear nuevo motor de EigenTrust
    pub fn new(config: ReputationConfig, pre_trusted: Vec<WalletAddress>) -> Self {
        Self {
            config,
            pre_trusted,
            attestations: Vec::new(),
            last_result: None,
        }
    }

    /// Registrar una atestación de reputación
    pub fn add_attestation(&mut self, attestation: ReputationAttestation) {
        self.attestations.push(attestation);
    }

    /// Ejecutar cómputo EigenTrust completo
    ///
    /// 1. Construir matriz de confianza local normalizada
    /// 2. Power iteration con teletransporte
    /// 3. Distrust adjustment
    /// 4. Retornar scores
    pub fn compute(&mut self) -> Result<EigenTrustResult, ReputationError> {
        todo!("Feature 4.1 — EigenTrust compute")
    }

    /// Obtener trust score de una wallet
    pub fn trust_score(&self, wallet: &WalletAddress) -> Option<f64> {
        self.last_result
            .as_ref()
            .and_then(|r| r.scores.get(wallet).copied())
    }

    /// Calcular reputación híbrida (EigenTrust + Contribution)
    pub fn hybrid_score(&self, eigentrust_score: f64, contribution_score: f64) -> f64 {
        self.config.eigentrust_weight * eigentrust_score
            + self.config.contribution_weight * contribution_score
    }

    /// Detectar colusión (subgrafos densos de co-validación)
    ///
    /// Si A y B se validan mutuamente >80% del tiempo sin variación,
    /// ambos son marcados como potencial colusión.
    pub fn detect_collusion(&self) -> Vec<(WalletAddress, WalletAddress, f64)> {
        todo!("Feature 4.1 — Collusion detection")
    }
}

/// Calculador de contribution score
pub struct ContributionCalculator;

impl ContributionCalculator {
    /// Calcular contribution score de una wallet basado en:
    /// - # de contextos compartidos únicos
    /// - % de contextos comprados por otros (utilidad)
    /// - Uptime del nodo
    /// - Versión actualizada
    /// - Validaciones realizadas con acierto
    pub fn calculate(_wallet: &WalletAddress, _history: &ContributionHistory) -> u64 {
        todo!("Feature 4.2 — Contribution score")
    }
}

/// Historial de contribución de una wallet
#[derive(Debug, Clone, Default)]
pub struct ContributionHistory {
    /// Contextos compartidos (únicos, por hash)
    pub shared_contexts: Vec<String>,
    /// Contextos que fueron comprados por otros
    pub purchased_contexts: Vec<String>,
    /// Validaciones realizadas
    pub validations: Vec<ValidationRecord>,
    /// Uptime del nodo (en segundos)
    pub total_uptime: u64,
    /// Versión actual del nodo
    pub node_version: String,
}

#[derive(Debug, Clone)]
pub struct ValidationRecord {
    pub context_hash: String,
    pub was_correct: bool,
    pub timestamp: u64,
}

#[derive(Debug)]
pub enum ReputationError {
    NoAttestations,
    TooFewPeers,
    NotConverged,
    InvalidData,
}

impl std::fmt::Display for ReputationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAttestations => write!(f, "No hay atestaciones para computar EigenTrust"),
            Self::TooFewPeers => write!(f, "Muy pocos peers para un cómputo significativo"),
            Self::NotConverged => write!(f, "EigenTrust no convergió en el máximo de iteraciones"),
            Self::InvalidData => write!(f, "Datos de entrada inválidos para EigenTrust"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hybrid_score_default_weights() {
        let engine = EigenTrustEngine::new(ReputationConfig::default(), vec![]);
        let score = engine.hybrid_score(0.8, 0.6);
        // 0.7 * 0.8 + 0.3 * 0.6 = 0.56 + 0.18 = 0.74
        assert!((score - 0.74).abs() < 0.001);
    }
}
