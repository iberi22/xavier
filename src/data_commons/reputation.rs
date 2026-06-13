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
use std::collections::HashMap;

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
    pub fn compute(&mut self) -> Result<EigenTrustResult, ReputationError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if self.attestations.is_empty() {
            return Err(ReputationError::NoAttestations);
        }

        // 1. Recolectar todos los participantes
        let mut wallets: Vec<WalletAddress> = self
            .attestations
            .iter()
            .flat_map(|a| vec![a.from.clone(), a.to.clone()])
            .collect();
        wallets.extend(self.pre_trusted.clone());
        wallets.sort_by_key(|a| a.0.clone());
        wallets.dedup();

        let n = wallets.len();
        if n < 2 {
            return Err(ReputationError::TooFewPeers);
        }

        let wallet_to_idx: HashMap<WalletAddress, usize> = wallets
            .iter()
            .enumerate()
            .map(|(i, w)| (w.clone(), i))
            .collect();

        // 2. Construir matriz de confianza local normalizada
        let mut matrix = vec![vec![0.0; n]; n];
        for att in &self.attestations {
            let i = wallet_to_idx[&att.from];
            let j = wallet_to_idx[&att.to];
            if att.score > 0 {
                matrix[i][j] += att.score as f64;
            }
        }

        // Normalizar filas
        for i in 0..n {
            let sum: f64 = matrix[i].iter().sum();
            if sum > 0.0 {
                for j in 0..n {
                    matrix[i][j] /= sum;
                }
            } else {
                // Si no confía en nadie, confía en pre-trusted
                for pt in &self.pre_trusted {
                    if let Some(&j) = wallet_to_idx.get(pt) {
                        matrix[i][j] = 1.0 / self.pre_trusted.len() as f64;
                    }
                }
            }
        }

        // 3. Power iteration
        let mut t = vec![1.0 / n as f64; n];
        let p = {
            let mut pv = vec![0.0; n];
            if self.pre_trusted.is_empty() {
                pv.fill(1.0 / n as f64);
            } else {
                for pt in &self.pre_trusted {
                    if let Some(&idx) = wallet_to_idx.get(pt) {
                        pv[idx] = 1.0 / self.pre_trusted.len() as f64;
                    }
                }
            }
            pv
        };

        let a = self.config.teleport_factor;
        let mut iter = 0;
        loop {
            let mut t_next = vec![0.0; n];
            for j in 0..n {
                let mut sum = 0.0;
                for i in 0..n {
                    sum += matrix[i][j] * t[i];
                }
                t_next[j] = (1.0 - a) * sum + a * p[j];
            }

            // Verificar convergencia
            let diff: f64 = t.iter().zip(t_next.iter()).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt();
            t = t_next;
            iter += 1;

            if diff < self.config.convergence_threshold {
                let mut scores = HashMap::new();
                for (i, w) in wallets.into_iter().enumerate() {
                    scores.insert(w, t[i]);
                }
                let result = EigenTrustResult {
                    scores,
                    iterations: iter,
                    convergence_diff: diff,
                    computed_at: now,
                };
                self.last_result = Some(result.clone());
                return Ok(result);
            }

            if iter >= self.config.max_iterations {
                return Err(ReputationError::NotConverged);
            }
        }
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
            + self.config.contribution_weight * (contribution_score / 1000.0)
    }

    /// Detectar colusión (subgrafos densos de co-validación)
    pub fn detect_collusion(&self) -> Vec<(WalletAddress, WalletAddress, f64)> {
        let mut suspicious = Vec::new();
        // Implementación simplificada: buscar pares que se votan mutuamente con frecuencia
        let mut mutual_votes: HashMap<(WalletAddress, WalletAddress), u32> = HashMap::new();

        for att in &self.attestations {
            let key = if att.from.0 < att.to.0 {
                (att.from.clone(), att.to.clone())
            } else {
                (att.to.clone(), att.from.clone())
            };
            *mutual_votes.entry(key).or_insert(0) += 1;
        }

        for (pair, count) in mutual_votes {
            if count > 10 { // Threshold arbitrario para el MVP
                suspicious.push((pair.0, pair.1, count as f64));
            }
        }
        suspicious
    }
}

/// Calculador de contribution score
pub struct ContributionCalculator;

impl ContributionCalculator {
    /// Calcular contribution score de una wallet
    pub fn calculate(_wallet: &WalletAddress, history: &ContributionHistory) -> u64 {
        let mut score = 0;

        // 1. Contextos compartidos
        score += (history.shared_contexts.len() * 10) as u64;

        // 2. Utilidad (contextos comprados por otros)
        score += (history.purchased_contexts.len() * 50) as u64;

        // 3. Validaciones correctas
        let correct_validations = history.validations.iter().filter(|v| v.was_correct).count();
        score += (correct_validations * 20) as u64;

        // 4. Uptime (1 punto por hora, max 200)
        score += (history.total_uptime / 3600).min(200);

        score.min(1000) // Capeado a 1000
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
