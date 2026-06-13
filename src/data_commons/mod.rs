//! # Xavier Data Commons ($XAV)
//!
//! Sistema descentralizado de compartición de datos técnicos entre nodos Xavier.
//! - **Wallet post-cuántica:** ML-KEM (Kyber-1024) + ML-DSA (Dilithium-5)
//! - **TPM 2.0 opcional:** HW wallet cuando está disponible
//! - **Reputación EigenTrust:** Trust scoring descentralizado sin blockchain
//! - **MINTER automático:** Recompensas por contribución de datos técnicos
//! - **Gobernanza 100% democrática:** 1 wallet = 1 voto
//!
//! ## Filosofía
//!
//! > "Los datos técnicos de Xavier son el combustible para la evolución autónoma
//! > de toda la mesh. Quien contribuye es recompensado. Quien consume, paga.
//! > La red decide su futuro, no una entidad central."
//!
//! ## Token: $XAV
//!
//! - **Supply:** No fijo — se mintea solo con contribución válida
//! - **Quema:** 80% del precio pagado por consumir contextos se quema
//! - **Pre-mining:** 0 — arranca desde cero con cada nodo
//! - **Bridge:** No por ahora (independiente de GARA)
//!
//! ## Estado
//!
//! ⚠️ **FASE 0 — Diseño e investigación.** Sin implementación aún.
//! El código en este módulo define la estructura de datos y tipos,
//! pero las features están en documentos de diseño.

pub mod funnel;
pub mod governance;
pub mod reputation;
pub mod types;
#[cfg(feature = "post-quantum")]
pub mod wallet;

// Re-export principales
pub use types::*;
