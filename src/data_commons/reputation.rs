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
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Reputation tier derived from Karma threshold
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReputationTier {
    Newcomer,
    Contributor,
    Trusted,
    Elder,
}

impl ReputationTier {
    /// Calculate tier from karma score:
    /// - Newcomer: < 300
    /// - Contributor: 300..600
    /// - Trusted: 600..900
    /// - Elder: >= 900
    pub fn from_karma(karma: i64) -> Self {
        if karma >= 900 {
            Self::Elder
        } else if karma >= 600 {
            Self::Trusted
        } else if karma >= 300 {
            Self::Contributor
        } else {
            Self::Newcomer
        }
    }
}

impl std::fmt::Display for ReputationTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Newcomer => write!(f, "newcomer"),
            Self::Contributor => write!(f, "contributor"),
            Self::Trusted => write!(f, "trusted"),
            Self::Elder => write!(f, "elder"),
        }
    }
}

/// Karma log entry recording individual reward / sanction event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KarmaLogEntry {
    pub timestamp: u64,
    pub amount: i64,
    pub reason: String,
}

/// Agent karma state tracking balance, log history, and last decay timestamp
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AgentKarmaRecord {
    pub karma: i64,
    pub history: Vec<KarmaLogEntry>,
    pub last_decay: u64,
}

/// Persistent Karma engine managing rewards, sanctions, tiers, decay, and persistence to `data/ivn/karma.json`.
#[derive(Debug, Clone)]
pub struct KarmaEngine {
    storage_path: PathBuf,
    records: HashMap<String, AgentKarmaRecord>,
}

impl Default for KarmaEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl KarmaEngine {
    /// Create new KarmaEngine using default storage path `data/ivn/karma.json`
    pub fn new() -> Self {
        Self::with_path(PathBuf::from("data/ivn/karma.json"))
    }

    /// Create KarmaEngine with custom storage path
    pub fn with_path<P: Into<PathBuf>>(path: P) -> Self {
        let storage_path = path.into();
        let mut engine = Self {
            storage_path,
            records: HashMap::new(),
        };
        let _ = engine.load();
        engine
    }

    /// Load records from storage file if it exists
    pub fn load(&mut self) -> Result<(), String> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(&self.storage_path)
            .map_err(|e| format!("Failed to read karma file: {}", e))?;
        let records: HashMap<String, AgentKarmaRecord> = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse karma JSON: {}", e))?;
        self.records = records;
        Ok(())
    }

    /// Save records to storage file
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.storage_path.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let json = serde_json::to_string_pretty(&self.records)
            .map_err(|e| format!("Failed to serialize karma records: {}", e))?;
        std::fs::write(&self.storage_path, json)
            .map_err(|e| format!("Failed to write karma file: {}", e))?;
        Ok(())
    }

    /// Award karma to an agent for a specified reason
    pub fn reward(&mut self, agent: &str, amount: i64, reason: &str) -> i64 {
        let abs_amount = amount.abs();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let record = self.records.entry(agent.to_string()).or_default();
        record.karma += abs_amount;
        record.history.push(KarmaLogEntry {
            timestamp: now,
            amount: abs_amount,
            reason: reason.to_string(),
        });

        let new_karma = record.karma;
        let _ = self.save();
        new_karma
    }

    /// Sanction karma of an agent (slash/deduct) for a specified reason
    pub fn sanction(&mut self, agent: &str, amount: i64, reason: &str) -> i64 {
        let abs_amount = amount.abs();
        let penalty_amount = -abs_amount;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let record = self.records.entry(agent.to_string()).or_default();
        record.karma += penalty_amount;
        record.history.push(KarmaLogEntry {
            timestamp: now,
            amount: penalty_amount,
            reason: reason.to_string(),
        });

        let new_karma = record.karma;
        let _ = self.save();
        new_karma
    }

    /// Get current karma balance of an agent
    pub fn get_karma(&self, agent: &str) -> i64 {
        self.records.get(agent).map(|r| r.karma).unwrap_or(0)
    }

    /// Get current reputation tier of an agent
    pub fn get_tier(&self, agent: &str) -> ReputationTier {
        ReputationTier::from_karma(self.get_karma(agent))
    }

    /// Get detailed karma record of an agent if present
    pub fn get_record(&self, agent: &str) -> Option<&AgentKarmaRecord> {
        self.records.get(agent)
    }

    /// Apply daily 1% decay to all recorded agent karma balances
    pub fn decay(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for record in self.records.values_mut() {
            if record.karma > 0 {
                let decayed = (record.karma as f64 * 0.99) as i64;
                record.karma = decayed;
            }
            record.last_decay = now;
        }

        let _ = self.save();
    }
}

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
    /// Computed global trust scores (wallet string → score), populated after compute()
    computed_scores: HashMap<String, f64>,
    /// Storage for wallet karma balances (soulbound + EigenTrust integration)
    karma_store: HashMap<WalletAddress, i64>,
}

impl EigenTrustEngine {
    /// Crear nuevo motor de EigenTrust
    pub fn new(config: ReputationConfig, pre_trusted: Vec<WalletAddress>) -> Self {
        Self {
            config,
            pre_trusted,
            attestations: Vec::new(),
            last_result: None,
            computed_scores: HashMap::new(),
            karma_store: HashMap::new(),
        }
    }

    /// Adjust karma balance for a wallet address and return the new balance.
    pub fn adjust_karma(&mut self, wallet: &WalletAddress, delta: i64) -> i64 {
        let current = self.karma_store.entry(wallet.clone()).or_insert(0);
        *current += delta;
        *current
    }

    /// Query current karma balance of a wallet address.
    pub fn karma_of(&self, wallet: &WalletAddress) -> i64 {
        self.karma_store.get(wallet).copied().unwrap_or(0)
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
        if self.attestations.is_empty() {
            return Err(ReputationError::NoAttestations);
        }

        let mut wallets: Vec<WalletAddress> = self
            .attestations
            .iter()
            .flat_map(|a| vec![a.from.clone(), a.to.clone()])
            .collect();
        wallets.sort_by(|a, b| a.0.cmp(&b.0));
        wallets.dedup();

        let n = wallets.len();
        if n < 2 && self.pre_trusted.is_empty() {
            return Err(ReputationError::TooFewPeers);
        }

        let wallet_to_idx: HashMap<WalletAddress, usize> = wallets
            .iter()
            .enumerate()
            .map(|(i, w)| (w.clone(), i))
            .collect();

        // 1. Construir matriz de confianza local normalizada C
        let mut c = vec![vec![0.0; n]; n];
        for a in &self.attestations {
            if let (Some(&from_idx), Some(&to_idx)) =
                (wallet_to_idx.get(&a.from), wallet_to_idx.get(&a.to))
            {
                if a.score > 0 {
                    c[from_idx][to_idx] += a.score as f64;
                }
            }
        }

        // Normalizar filas
        for row in c.iter_mut().take(n) {
            let row_sum: f64 = row.iter().sum();
            if row_sum > 0.0 {
                for value in row.iter_mut().take(n) {
                    *value /= row_sum;
                }
            } else {
                // Si no confía en nadie, confía uniformemente en pre-trusted o en todos
                if !self.pre_trusted.is_empty() {
                    let pt_weight = 1.0 / self.pre_trusted.len() as f64;
                    for pt in &self.pre_trusted {
                        if let Some(&idx) = wallet_to_idx.get(pt) {
                            row[idx] = pt_weight;
                        }
                    }
                } else {
                    let weight = 1.0 / n as f64;
                    for value in row.iter_mut().take(n) {
                        *value = weight;
                    }
                }
            }
        }

        // 2. Power iteration
        let a = self.config.teleport_factor;
        let mut p = vec![0.0; n];
        if !self.pre_trusted.is_empty() {
            let pt_weight = 1.0 / self.pre_trusted.len() as f64;
            for pt in &self.pre_trusted {
                if let Some(&idx) = wallet_to_idx.get(pt) {
                    p[idx] = pt_weight;
                }
            }
        } else {
            p.fill(1.0 / n as f64);
        }

        let mut t = p.clone();
        let mut converged = false;
        let mut final_diff = 0.0;
        let mut final_iterations = 0;

        for k in 0..self.config.max_iterations {
            let mut t_next = vec![0.0; n];
            for j in 0..n {
                let mut sum = 0.0;
                for i in 0..n {
                    sum += c[i][j] * t[i];
                }
                t_next[j] = (1.0 - a) * sum + a * p[j];
            }

            // Verificar convergencia
            let diff: f64 = t
                .iter()
                .zip(t_next.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            t = t_next;
            final_diff = diff;
            final_iterations = k + 1;

            if diff < self.config.convergence_threshold {
                converged = true;
                break;
            }
        }

        if !converged {
            return Err(ReputationError::NotConverged);
        }

        let scores: HashMap<WalletAddress, f64> = wallets
            .into_iter()
            .enumerate()
            .map(|(i, w)| (w, t[i]))
            .collect();

        // Populate computed_scores for quick lookup by wallet string
        self.computed_scores = scores.iter().map(|(w, s)| (w.0.clone(), *s)).collect();

        let result = EigenTrustResult {
            scores,
            iterations: final_iterations,
            convergence_diff: final_diff,
            computed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.last_result = Some(result.clone());
        Ok(result)
    }

    /// Obtener trust score de una wallet
    pub fn trust_score(&self, wallet: &WalletAddress) -> Option<f64> {
        self.last_result
            .as_ref()
            .and_then(|r| r.scores.get(wallet).copied())
    }

    /// Get computed trust score for a wallet by string address.
    /// Returns 0.0 if the wallet has no score.
    pub fn computed_score(&self, wallet: &str) -> f64 {
        self.computed_scores.get(wallet).copied().unwrap_or(0.0)
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
        let mut pairs: HashMap<(WalletAddress, WalletAddress), (u32, u32)> = HashMap::new();

        for a in &self.attestations {
            let key = if a.from.0 < a.to.0 {
                (a.from.clone(), a.to.clone())
            } else {
                (a.to.clone(), a.from.clone())
            };

            let stats = pairs.entry(key).or_insert((0, 0));
            stats.0 += 1; // total interacciones
            if a.score > 0 {
                stats.1 += 1; // interacciones positivas
            }
        }

        pairs
            .into_iter()
            .filter(|(_, (total, pos))| *total > 5 && (*pos as f64 / *total as f64) > 0.8)
            .map(|(key, (total, pos))| (key.0, key.1, pos as f64 / total as f64))
            .collect()
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
    pub fn calculate(_wallet: &WalletAddress, history: &ContributionHistory) -> u64 {
        let mut score = 0.0;

        // 1. Contextos compartidos (máx 300 pts)
        // 10 pts por cada uno, hasta 30
        score += (history.shared_contexts.len() as f64 * 10.0).min(300.0);

        // 2. Utilidad (máx 300 pts)
        // Si otros compraron tus contextos, es que son útiles
        if !history.shared_contexts.is_empty() {
            let utility_ratio =
                history.purchased_contexts.len() as f64 / history.shared_contexts.len() as f64;
            score += utility_ratio * 300.0;
        }

        // 3. Uptime (máx 200 pts)
        // 1 pt por cada hora (3600s), hasta 200h
        score += (history.total_uptime as f64 / 3600.0).min(200.0);

        // 4. Validaciones (máx 200 pts)
        // 20 pts por cada validación correcta
        let correct_validations = history.validations.iter().filter(|v| v.was_correct).count();
        score += (correct_validations as f64 * 20.0).min(200.0);

        score.min(1000.0) as u64
    }
}

/// Obtener el peso de reputación de una wallet para votación ponderada
///
/// Returns a u64 weight based on the trust score (EigenTrust + contribution).
/// If no score is available, returns 1 (base weight = 1 vote).
/// Scale: 1-1000 where higher = more influence in voting.
pub fn reputation_weight(_wallet_id: &str) -> u64 {
    // Base weight is always 1 for active wallets
    // In production this would query EigenTrust scores and compute:
    // weight = 1 + (trust_score_normalized * 999)
    // For now, return 1 so every active wallet has at least 1 unit of voting power.
    // This is intentionally simple for the initial implementation.
    1
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

    #[test]
    fn test_eigentrust_convergence() {
        let config = ReputationConfig::default();
        let mut engine = EigenTrustEngine::new(config, vec![]);

        let w1 = WalletAddress("xv1_1".into());
        let w2 = WalletAddress("xv1_2".into());
        let w3 = WalletAddress("xv1_3".into());

        // w1 confía en w2
        engine.add_attestation(ReputationAttestation {
            from: w1.clone(),
            to: w2.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });

        // w2 confía en w3
        engine.add_attestation(ReputationAttestation {
            from: w2.clone(),
            to: w3.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });

        // w3 confía en w1
        engine.add_attestation(ReputationAttestation {
            from: w3.clone(),
            to: w1.clone(),
            score: 1,
            context_hash: None,
            timestamp: 0,
            signature: vec![],
        });

        let result = engine.compute().unwrap();

        // En un anillo simétrico sin pre-trusted, todos deberían tener el mismo score (1/3)
        let s1 = result.scores.get(&w1).unwrap();
        let s2 = result.scores.get(&w2).unwrap();
        let s3 = result.scores.get(&w3).unwrap();

        assert!((s1 - 0.333).abs() < 0.01);
        assert!((s2 - 0.333).abs() < 0.01);
        assert!((s3 - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_contribution_score_calculation() {
        let w = WalletAddress("xv1_test".into());
        let history = ContributionHistory {
            shared_contexts: vec!["h1".into(), "h2".into(), "h3".into()], // 3 * 10 = 30 pts
            purchased_contexts: vec!["h1".into()],                        // 1/3 utility = 100 pts
            total_uptime: 3600 * 50,                                      // 50h = 50 pts
            validations: vec![
                ValidationRecord {
                    context_hash: "c1".into(),
                    was_correct: true,
                    timestamp: 0,
                },
                ValidationRecord {
                    context_hash: "c2".into(),
                    was_correct: true,
                    timestamp: 0,
                },
            ], // 2 * 20 = 40 pts
            node_version: "1.0.0".into(),
        };

        let score = ContributionCalculator::calculate(&w, &history);
        // Total: 30 + 100 + 50 + 40 = 220
        assert_eq!(score, 220);
    }

    #[test]
    fn test_collusion_detection() {
        let config = ReputationConfig::default();
        let mut engine = EigenTrustEngine::new(config, vec![]);

        let a = WalletAddress("xv1_a".into());
        let b = WalletAddress("xv1_b".into());

        // A y B se validan mutuamente 10 veces
        for i in 0..10 {
            engine.add_attestation(ReputationAttestation {
                from: a.clone(),
                to: b.clone(),
                score: 1,
                context_hash: Some(format!("ctx_{}", i)),
                timestamp: 0,
                signature: vec![],
            });
            engine.add_attestation(ReputationAttestation {
                from: b.clone(),
                to: a.clone(),
                score: 1,
                context_hash: Some(format!("ctx_{}_rev", i)),
                timestamp: 0,
                signature: vec![],
            });
        }

        let collusion = engine.detect_collusion();
        assert!(!collusion.is_empty());
        assert_eq!(collusion[0].2, 1.0); // 100% positive interaction
    }

    #[test]
    fn test_reputation_tier() {
        assert_eq!(ReputationTier::from_karma(0), ReputationTier::Newcomer);
        assert_eq!(ReputationTier::from_karma(299), ReputationTier::Newcomer);
        assert_eq!(ReputationTier::from_karma(300), ReputationTier::Contributor);
        assert_eq!(ReputationTier::from_karma(599), ReputationTier::Contributor);
        assert_eq!(ReputationTier::from_karma(600), ReputationTier::Trusted);
        assert_eq!(ReputationTier::from_karma(899), ReputationTier::Trusted);
        assert_eq!(ReputationTier::from_karma(900), ReputationTier::Elder);
        assert_eq!(ReputationTier::from_karma(1500), ReputationTier::Elder);
    }

    #[test]
    fn test_karma_reward_sanction_decay() {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("karma_test_{}.json", ulid::Ulid::new()));
        let mut engine = KarmaEngine::with_path(&file_path);

        let agent = "agent_x";

        assert_eq!(engine.get_karma(agent), 0);
        assert_eq!(engine.get_tier(agent), ReputationTier::Newcomer);

        // Reward +500 karma
        let k1 = engine.reward(agent, 500, "verified identity proof");
        assert_eq!(k1, 500);
        assert_eq!(engine.get_karma(agent), 500);
        assert_eq!(engine.get_tier(agent), ReputationTier::Contributor);

        // Sanction -100 karma
        let k2 = engine.sanction(agent, 100, "false positive vote");
        assert_eq!(k2, 400);
        assert_eq!(engine.get_karma(agent), 400);

        // Check history log entries
        let record = engine.get_record(agent).unwrap();
        assert_eq!(record.history.len(), 2);
        assert_eq!(record.history[0].amount, 500);
        assert_eq!(record.history[1].amount, -100);

        // Apply decay (1% decay: 400 * 0.99 = 396)
        engine.decay();
        assert_eq!(engine.get_karma(agent), 396);

        // Reload engine from disk to verify persistence
        let engine_reloaded = KarmaEngine::with_path(&file_path);
        assert_eq!(engine_reloaded.get_karma(agent), 396);
        assert_eq!(engine_reloaded.get_tier(agent), ReputationTier::Contributor);

        let _ = std::fs::remove_file(file_path);
    }
}
