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
#[derive(Debug, Clone)]
pub struct FunnelConfig {
    /// Parámetros del sistema (gobernables)
    pub params: SystemParams,
    /// Rate limiting por wallet
    pub rate_limits: RateLimits,
}

impl Default for FunnelConfig {
    fn default() -> Self {
        Self {
            params: SystemParams::default(),
            rate_limits: RateLimits::default(),
        }
    }
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
    pub fn validate_context(&self, offer: &ContextOffer) -> Result<(), MinterError> {
        // 1. No duplicado
        if self.context_history.contains(&offer.context_hash) {
            return Err(MinterError::DuplicateContext);
        }

        // 2. Rate limit
        if !self.check_rate_limit(&offer.seller_address, offer.seller_trust) {
            return Err(MinterError::RateLimitExceeded);
        }

        // 3. Nodo tiene Proof of Liveliness (se asume validado por el nodo llamante por ahora)

        Ok(())
    }

    /// Calcular la recompensa por un contexto
    pub fn calculate_reward(&self, offer: &ContextOffer) -> RewardBreakdown {
        let base_price = self.config.params.reference_price;
        let rarity_multiplier = (1.0 / offer.rarity.max(0.01)).min(10.0);

        // Normalizar trust score (-1000 a 1000) -> (0.1 a 1.0)
        let trust_multiplier = (offer.seller_trust as f32 + 1000.0) / 2000.0 * 0.9 + 0.1;

        let category_key = format!("{:?}", offer.category);
        let category_multiplier = *self.config.params.category_multipliers.get(&category_key).unwrap_or(&1.0);

        let final_amount = (base_price as f32 * rarity_multiplier * trust_multiplier * category_multiplier) as u64;

        let split = self.config.params.reward_split;
        RewardBreakdown {
            node_reward: (final_amount * split[0] as u64) / 100,
            wallet_reward: (final_amount * split[1] as u64) / 100,
            network_reserve: (final_amount * split[2] as u64) / 100,
            factors: RewardFactors {
                base_price,
                rarity_multiplier,
                trust_multiplier,
                category_multiplier,
                final_amount,
            },
        }
    }

    /// Ejecutar minteo: crear evento de emisión
    pub fn mint(&mut self, offer: &ContextOffer) -> Result<MinterEvent, MinterError> {
        self.validate_context(offer)?;

        let breakdown = self.calculate_reward(offer);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generar una firma del sistema (simulada con HMAC para el MVP)
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(b"xavier-system-secret-key")
            .map_err(|_| MinterError::InvalidContext)?;
        mac.update(offer.context_hash.as_bytes());
        mac.update(&now.to_le_bytes());
        let signature = mac.finalize().into_bytes().to_vec();

        let event = MinterEvent {
            tx_hash: format!("tx_{}_{}", offer.context_hash, now),
            beneficiary: offer.seller_address.clone(),
            amount: breakdown.factors.final_amount,
            breakdown,
            minted_at: now,
            signature,
        };

        self.context_history.insert(offer.context_hash.clone());
        self.mint_history.push(event.clone());

        Ok(event)
    }

    /// Quemar tokens al comprar un contexto
    pub fn burn(
        &mut self,
        buyer: &WalletAddress,
        amount: u64,
        context_hash: &str,
    ) -> Result<BurnEvent, MinterError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Ok(BurnEvent {
            tx_hash: format!("burn_{}_{}", context_hash, now),
            burner: buyer.clone(),
            amount: (amount * self.config.params.burn_rate as u64) / 100,
            context_hash: context_hash.to_string(),
            burned_at: now,
            signature: Vec::new(),
        })
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
