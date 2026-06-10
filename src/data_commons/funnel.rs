//! # Funnel de Recompensas — MINTER + BURN
//!
//! ## MINTER Automático
//!
//! Se activa cuando un nodo comparte un contexto técnico válido.
//!
//! ### Cálculo de Recompensa
//!
//! ```text
//! Precio = PrecioReferencia × (1 / Rareza) × TrustScore × MultiplicadorTipo
//!
//! Split: 40% nodo + 40% wallet + 20% red
//! Quema al comprar: 80% del precio + 20% a rewards pool
//! ```
//!
//! ### Anti-Manipulación
//!
//! - **Rate limiting:** Máx 10 contextos/día para trust < 0.3
//! - **Sin duplicados:** Hash SHA-256 único por contexto
//! - **Proof of Liveliness:** Nodo debe tener ≥24h de uptime
//! - **Self-dealing detection:** Mismo seed → rechazar
//! - **Collusion detection:** Subgrafos densos de co-validación

use crate::data_commons::types::*;
use std::collections::HashSet;

/// Configuración del funnel de recompensas
#[derive(Debug, Clone, Default)]
pub struct FunnelConfig {
    /// Parámetros del sistema (gobernables)
    pub params: SystemParams,
    /// Rate limiting por wallet
    pub rate_limits: RateLimits,
}

#[derive(Debug, Clone)]
pub struct RateLimits {
    /// Máx contextos/día por wallet (general)
    pub daily_per_wallet: u32,
    /// Máx contextos/día para wallets con trust < threshold
    pub daily_per_low_trust: u32,
    /// Threshold de trust para aplicar rate limit bajo
    pub low_trust_threshold: i64,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            daily_per_wallet: 50,
            daily_per_low_trust: 10,
            low_trust_threshold: 300, // trust_score < 0.3 en escala -1000 a 1000
        }
    }
}

/// MINTER — emisor automático de $XAV
pub struct Minter {
    config: FunnelConfig,
    /// Historial de contextos compartidos (para detectar duplicados)
    #[allow(dead_code)]
    context_history: HashSet<String>,
    /// Historial de minteos por wallet (rate limiting)
    mint_history: Vec<MinterEvent>,
}

impl Minter {
    pub fn new(config: FunnelConfig) -> Self {
        Self {
            config,
            context_history: HashSet::new(),
            mint_history: Vec::new(),
        }
    }

    /// Evaluar si un contexto es válido para mintear
    ///
    /// Verifica:
    /// 1. No duplicado (hash único)
    /// 2. Rate limit no excedido
    /// 3. Nodo tiene Proof of Liveliness (≥24h uptime)
    /// 4. No self-dealing (el comprador no es el vendedor)
    /// 5. No collusion flag activo
    pub fn validate_context(&self, _offer: &ContextOffer) -> Result<(), MinterError> {
        todo!("Feature 3.1 — Validate context for minting")
    }

    /// Calcular la recompensa por un contexto
    ///
    /// Fórmula:
    /// ```text
    /// Precio = PrecioReferencia × (1 / max(Rareza, 0.01)) × TrustScoreNormalized × MultiplicadorTipo
    /// ```
    pub fn calculate_reward(&self, _offer: &ContextOffer) -> RewardBreakdown {
        todo!("Feature 3.1 — Calculate reward")
    }

    /// Ejecutar minteo: crear evento de emisión
    pub fn mint(&mut self, _offer: &ContextOffer) -> Result<MinterEvent, MinterError> {
        todo!("Feature 3.1 — Execute mint")
    }

    /// Quemar tokens al comprar un contexto
    ///
    /// 80% del precio se quema (envía a address burn)
    /// 20% va a rewards pool
    pub fn burn(
        &mut self,
        _buyer: &WalletAddress,
        _amount: u64,
        _context_hash: &str,
    ) -> Result<BurnEvent, MinterError> {
        todo!("Feature 3.3 — Execute burn")
    }

    /// Verificar rate limit
    pub fn check_rate_limit(&self, wallet: &WalletAddress, trust_score: i64) -> bool {
        // Calcular cuántos contextos ha compartido esta wallet hoy
        let today = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            / 86400;

        let count_today = self
            .mint_history
            .iter()
            .filter(|e| e.beneficiary == *wallet && e.minted_at / 86400 == today)
            .count() as u32;

        let limit = if trust_score < self.config.rate_limits.low_trust_threshold {
            self.config.rate_limits.daily_per_low_trust
        } else {
            self.config.rate_limits.daily_per_wallet
        };

        count_today < limit
    }
}

#[derive(Debug)]
pub enum MinterError {
    DuplicateContext,
    RateLimitExceeded,
    InsufficientUptime,
    SelfDealing,
    CollusionDetected,
    InvalidContext,
    BurnFailed,
}

impl std::fmt::Display for MinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateContext => write!(f, "Este contexto ya existe en la red"),
            Self::RateLimitExceeded => write!(f, "Límite diario de contextos excedido"),
            Self::InsufficientUptime => write!(f, "El nodo necesita ≥24h de uptime para mintear"),
            Self::SelfDealing => write!(f, "No puedes comprar tu propio contexto"),
            Self::CollusionDetected => write!(f, "Colusión detectada — transacción rechazada"),
            Self::InvalidContext => write!(f, "El contexto no es válido o está corrupto"),
            Self::BurnFailed => write!(f, "Error al quemar tokens"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_high_trust() {
        let config = FunnelConfig::default();
        let minter = Minter::new(config);
        // Wallet con trust alto debería tener límite de 50/día
        assert!(minter.check_rate_limit(
            &WalletAddress("xv1_test".into()),
            800, // trust alto
        ));
    }

    #[test]
    fn test_rate_limit_low_trust() {
        let config = FunnelConfig::default();
        let minter = Minter::new(config);
        // Wallet con trust bajo debería tener límite de 10/día
        assert!(minter.check_rate_limit(
            &WalletAddress("xv1_test_low".into()),
            100, // trust bajo
        ));
    }
}
