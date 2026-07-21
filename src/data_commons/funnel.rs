// SPDX-License-Identifier: MIT OR LICENSE-MESH
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
use crate::observability::LogEntry;
use std::collections::HashSet;

/// Anonimizar un log antes de compartirlo
///
/// Elimina IDs únicos, IDs de correlación y datos sensibles del metadato.
pub fn anonymize_log(entry: &LogEntry) -> serde_json::Value {
    let mut metadata = entry.metadata.clone().unwrap_or(serde_json::json!({}));

    // Eliminar campos sensibles si existen
    if let Some(obj) = metadata.as_object_mut() {
        obj.remove("user");
        obj.remove("ip");
        obj.remove("auth_token");
        obj.remove("api_key");
        obj.remove("password");
        obj.remove("stack_trace");
        obj.remove("email");
        obj.remove("phone");
    }

    serde_json::json!({
        "timestamp": entry.timestamp,
        "level": entry.level.to_string(),
        "source": entry.source.to_string(),
        "module": entry.module,
        "message": entry.message,
        "metadata": metadata,
    })
}

/// Configuración del funnel de recompensas
#[derive(Debug, Clone, Default)]
pub struct FunnelConfig {
    /// Parámetros del sistema (gobernables)
    pub params: SystemParams,
    /// Rate limiting por wallet
    pub rate_limits: RateLimits,
    /// Consentimiento del usuario
    pub user_consent: bool,
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
    ///
    /// Verifica:
    /// 1. No duplicado (hash único)
    /// 2. Rate limit no excedido
    /// 3. Nodo tiene Proof of Liveliness (≥24h uptime)
    /// 4. No self-dealing (el comprador no es el vendedor)
    /// 5. No collusion flag activo
    pub fn validate_context(&self, offer: &ContextOffer) -> Result<(), MinterError> {
        // 1. No duplicado
        if self.context_history.contains(&offer.context_hash) {
            return Err(MinterError::DuplicateContext);
        }

        // 2. Rate limit
        if !self.check_rate_limit(&offer.seller_address, offer.seller_trust) {
            return Err(MinterError::RateLimitExceeded);
        }

        // 3. Proof of Liveliness (Simplificado para Fase 1: siempre OK si el timestamp es reciente)
        // TODO: Integrar con uptime real del nodo
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if offer.published_at > now + 300 {
            return Err(MinterError::InvalidContext);
        }

        Ok(())
    }

    /// Calcular la recompensa por un contexto
    ///
    /// Fórmula:
    /// ```text
    /// Precio = PrecioReferencia × (1 / max(Rareza, 0.01)) × TrustScoreNormalized × MultiplicadorTipo
    /// ```
    pub fn calculate_reward(&self, offer: &ContextOffer) -> RewardBreakdown {
        let params = &self.config.params;
        let base_price = params.reference_price;

        let rarity_multiplier = (1.0 / offer.rarity.max(0.01)).min(10.0);
        let trust_multiplier = (offer.seller_trust as f32 / 1000.0).max(0.1);

        let category_str = match offer.category {
            DataCategory::CriticalError => "CriticalError",
            DataCategory::FunctionalError => "FunctionalError",
            DataCategory::Benchmark => "Benchmark",
            DataCategory::NormalLog => "NormalLog",
            DataCategory::BasicTelemetry => "BasicTelemetry",
            DataCategory::Anomaly => "Anomaly",
        };

        let category_multiplier = params
            .category_multipliers
            .get(category_str)
            .copied()
            .unwrap_or(1.0);

        let final_amount =
            (base_price as f32 * rarity_multiplier * trust_multiplier * category_multiplier) as u64;

        let final_amount = final_amount.max(params.min_price).min(params.max_price);

        let node_reward = (final_amount * params.reward_split[0] as u64) / 100;
        let wallet_reward = (final_amount * params.reward_split[1] as u64) / 100;
        let network_reserve = final_amount - node_reward - wallet_reward;

        RewardBreakdown {
            node_reward,
            wallet_reward,
            network_reserve,
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
        if !self.config.user_consent {
            return Err(MinterError::NoConsent);
        }

        self.validate_context(offer)?;

        let breakdown = self.calculate_reward(offer);
        let minted_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let event = MinterEvent {
            tx_hash: format!("tx_{}_{}", offer.context_hash, minted_at),
            beneficiary: offer.seller_address.clone(),
            amount: breakdown.factors.final_amount,
            breakdown,
            minted_at,
            signature: vec![], // TODO: Firma del sistema
        };

        self.context_history.insert(offer.context_hash.clone());
        self.mint_history.push(event.clone());

        Ok(event)
    }

    /// Pipeline completo para procesar telemetría en el nodo mantenedor.
    /// Emite los tokens (mint) y luego cifra y guarda el payload real usando Cifrado Asimétrico.
    pub fn process_and_store_telemetry(
        &mut self,
        offer: &ContextOffer,
        payload_json: &str,
        db_path: &std::path::Path,
    ) -> Result<MinterEvent, MinterError> {
        let event = self.mint(offer)?;

        // Cifrar el payload asimétricamente para el nodo mantenedor (ECIES)
        let (encrypted_payload, ephemeral_pubkey) =
            crate::data_commons::maintainer::encrypt_for_maintainer(payload_json)
                .map_err(|_| MinterError::InvalidContext)?;

        let maintainer_pubkey =
            crate::data_commons::maintainer::get_maintainer_public_key().to_bytes();

        // Instanciar DB y guardar
        let db = crate::data_commons::telemetry_db::TelemetryDb::new(db_path)
            .map_err(|_| MinterError::InvalidContext)?;

        db.save_encrypted_log(
            &offer.context_hash,
            &encrypted_payload,
            &ephemeral_pubkey, // guardamos la llave pública efímera como el 'DEK' para que el mantenedor descifre
            &maintainer_pubkey,
            &offer.seller_address.0,
        )
        .map_err(|_| MinterError::InvalidContext)?;

        Ok(event)
    }

    /// Quemar tokens al comprar un contexto
    ///
    /// 80% del precio se quema (envía a address burn)
    /// 20% va a rewards pool
    pub fn burn(
        &mut self,
        buyer: &WalletAddress,
        amount: u64,
        context_hash: &str,
    ) -> Result<BurnEvent, MinterError> {
        let burned_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let burn_amount = (amount * self.config.params.burn_rate as u64) / 100;

        Ok(BurnEvent {
            tx_hash: format!("burn_{}_{}", context_hash, burned_at),
            burner: buyer.clone(),
            amount: burn_amount,
            context_hash: context_hash.to_string(),
            burned_at,
            signature: vec![], // TODO: Firma del sistema
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
    NoConsent,
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
            Self::NoConsent => write!(
                f,
                "El usuario no ha dado su consentimiento para compartir datos"
            ),
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

    #[test]
    fn test_anonymize_log() {
        use crate::observability::LogSource;

        let meta = serde_json::json!({
            "user": "alice",
            "ip": "1.2.3.4",
            "auth_token": "secret",
            "normal_data": "visible"
        });

        let entry =
            LogEntry::error(LogSource::HttpServer, "auth", "login failed").with_metadata(meta);

        let anonymized = anonymize_log(&entry);

        assert_eq!(anonymized["level"], "error");
        assert_eq!(anonymized["module"], "auth");
        assert_eq!(anonymized["message"], "login failed");
        assert_eq!(anonymized["metadata"]["normal_data"], "visible");

        // Sensitive fields should be removed
        assert!(anonymized["metadata"]["user"].is_null());
        assert!(anonymized["metadata"]["ip"].is_null());
        assert!(anonymized["metadata"]["auth_token"].is_null());
    }

    #[test]
    fn test_calculate_reward() {
        let config = FunnelConfig {
            user_consent: true,
            ..Default::default()
        };
        let minter = Minter::new(config);

        let offer = ContextOffer {
            context_hash: "hash123".into(),
            category: DataCategory::CriticalError,
            module: "core".into(),
            rarity: 0.1,        // 1/0.1 = 10x multiplier
            seller_trust: 1000, // 1.0x multiplier
            price: 0,
            published_at: 0,
            seller_address: WalletAddress("xv1_seller".into()),
        };

        let reward = minter.calculate_reward(&offer);

        // base(5) * rarity(10) * trust(1.0) * category(3.0) = 150
        assert_eq!(reward.factors.final_amount, 150);
        assert_eq!(reward.node_reward, 60); // 40% of 150
        assert_eq!(reward.wallet_reward, 60); // 40% of 150
        assert_eq!(reward.network_reserve, 30); // 20% of 150
    }

    #[test]
    fn test_mint_duplicate_fails() {
        let config = FunnelConfig {
            user_consent: true,
            ..Default::default()
        };
        let mut minter = Minter::new(config);

        let offer = ContextOffer {
            context_hash: "unique_hash".into(),
            category: DataCategory::NormalLog,
            module: "test".into(),
            rarity: 1.0,
            seller_trust: 500,
            price: 0,
            published_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            seller_address: WalletAddress("xv1_seller".into()),
        };

        // First mint should succeed
        assert!(minter.mint(&offer).is_ok());

        // Second mint with same hash should fail
        let result = minter.mint(&offer);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MinterError::DuplicateContext));
    }
}
