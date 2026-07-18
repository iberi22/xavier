//! # Xavier Data Commons ($SWAL / legacy $XAV naming)
//!
//! Sistema descentralizado de compartición de datos técnicos entre nodos Xavier
//! y aplicaciones del ecosistema SWAL.
//! - **Wallet post-cuántica:** ML-KEM (Kyber-1024) + ML-DSA (Dilithium-5)
//! - **TPM 2.0 opcional:** HW wallet cuando está disponible
//! - **Reputación EigenTrust:** Trust scoring descentralizado
//! - **Rewards / MINTER:** alineados al token de ecosistema **$SWAL**
//! - **Pro en apps:** nodo SWAL activo — **sin Stripe**
//! - **Namespaces multi-app:** `app/{app_id}/instance/{instance_id}/…`
//!
//! ## Filosofía
//!
//! > "Los datos técnicos y de trabajo con consentimiento son el combustible de la
//! > mesh SWAL. Quien contribuye es recompensado. Quien consume, paga.
//! > La propiedad de $SWAL es de las personas; el stake genera % de interés de red."
//!
//! ## Token: $SWAL
//!
//! - **Ownership:** wallet del usuario
//! - **Yield:** stake (economic core, donor gara-g)
//! - **Burn/mint:** equilibrio tipo BME en consumo vs contribución
//! - Naming legacy `$XAV` en tipos/docs → migrar a $SWAL
//!
//! ## Estado
//!
//! ⚠️ **FASE 0–1.** Tipos + wallet parcial; marketplace multi-app en roadmap.
//! Ver monorepo `docs/SWAL/README.md`.

pub mod funnel;
pub mod governance;
pub mod maintainer;
pub mod readiness;
pub mod reputation;
pub mod telemetry_db;
pub mod training;
pub mod types;
#[cfg(feature = "post-quantum")]
pub mod wallet;
