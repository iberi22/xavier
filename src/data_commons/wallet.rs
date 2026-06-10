//! # Wallet Post-Cuántica $XAV
//!
//! ## Stack Criptográfico
//!
//! | Propósito | Algoritmo | Crate |
//! |-----------|-----------|-------|
//! | Cifrado asimétrico | ML-KEM-1024 (Kyber-1024) | `oqs` |
//! | Firmas | ML-DSA-87 (Dilithium-5) | `oqs` |
//! | Identidad mesh | Ed25519 | `ed25519-dalek` (existente) |
//! | Seed phrase | BIP-39 español 24 palabras | `bip39` |
//! | Cifrado local | AES-256-GCM | `aes-gcm` |
//! | Key derivation | Argon2id | `argon2` |
//! | TPM (opcional) | RSA-2048 + SRK | `tpm-rs` |
//!
//! ## Key Hierarchy
//!
//! ```text
//! Seed (24 palabras BIP-39)
//!  ├── Argon2id → Wallet Master Key
//!  ├── ML-KEM-1024 keypair  (cifrado/descifrado de contextos)
//!  ├── ML-DSA-87 keypair    (firma de transacciones)
//!  └── Ed25519 keypair      (identidad mesh, ya existente)
//! ```
//!
//! ## TPM Flow (cuando disponible)
//!
//! ```text
//! TPM 2.0
//!  └── SRK (RSA-2048, almacenada en TPM, NUNCA sale)
//!       └── Wallet Key (AES-256 cifrada por SRK)
//!            └── Seed derivada dentro del TPM
//! ```
//!
//! ## Wallet Address
//!
//! Formato: `xv1_` + bech32(hash(ML-DSA-87 public key)[..32])
//! Longitud: 65 caracteres
//!
//! ## Múltiples Nodos
//!
//! Un wallet puede registrar N nodos. Cada nodo tiene su NodeID único.
//! El wallet firma el registro: (NodeID + WalletAddress) con Dilithium-5.
//! Las recompensas se acumulan por nodo y liquidan al wallet.

use crate::data_commons::types::*;
use std::path::PathBuf;

/// Configuración de wallet
#[derive(Debug, Clone)]
pub struct WalletConfig {
    /// Directorio donde almacenar datos de wallet
    pub data_dir: PathBuf,
    /// Usar TPM 2.0 si disponible?
    pub prefer_tpm: bool,
    /// Idioma de seed phrase (default: español)
    pub seed_language: String,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("~/.config/xavier/wallet"),
            prefer_tpm: true,
            seed_language: "spanish".into(),
        }
    }
}

/// Wallet post-cuántica $XAV
pub struct XavierWallet {
    /// Configuración
    pub config: WalletConfig,
    /// Estado de la wallet (si está cargada)
    pub state: Option<Wallet>,
    /// Usando TPM?
    pub has_tpm: bool,
}

impl XavierWallet {
    /// Crear una nueva wallet desde seed phrase
    ///
    /// # Flow
    /// 1. Derivat seed phrase → master key (Argon2id)
    /// 2. Generar ML-KEM-1024 keypair
    /// 3. Generar ML-DSA-87 keypair
    /// 4. Generar Ed25519 keypair (mesh)
    /// 5. Derivar wallet address: xv1_ + bech32(hash(ML-DSA-87 pk))
    /// 6. Si TPM disponible, cifrar seed con SRK
    /// 7. Si no TPM, cifrar seed con AES-256-GCM + contraseña
    /// 8. Persistir keypairs cifrados en disco
    /// 9. Retornar Wallet + seed phrase
    pub fn create(_config: WalletConfig) -> Result<(Self, String), WalletError> {
        todo!("Feature 1.1 — Wallet Creation")
    }

    /// Importar wallet desde seed phrase
    pub fn from_seed(_seed_phrase: &str, _config: WalletConfig) -> Result<Self, WalletError> {
        todo!("Feature 1.1 — Import wallet")
    }

    /// Importar wallet desde QR code (imagen)
    pub fn from_qr(_qr_image_path: &str, _config: WalletConfig) -> Result<Self, WalletError> {
        todo!("Feature 1.1 — Import wallet from QR")
    }

    /// Cargar wallet existente desde disco
    pub fn load(_config: WalletConfig) -> Result<Self, WalletError> {
        todo!("Feature 1.1 — Load wallet")
    }

    /// Firmar datos con ML-DSA-87
    pub fn sign(&self, _data: &[u8]) -> Result<Vec<u8>, WalletError> {
        todo!("Feature 1.1 — Sign with Dilithium-5")
    }

    /// Verificar firma ML-DSA-87
    pub fn verify(
        &self,
        _data: &[u8],
        _signature: &[u8],
        _public_key: &[u8],
    ) -> Result<bool, WalletError> {
        todo!("Feature 1.1 — Verify Dilithium-5 signature")
    }

    /// Cifrar datos para un destinatario (ML-KEM-1024)
    pub fn encrypt(
        &self,
        _data: &[u8],
        _recipient_public_key: &[u8],
    ) -> Result<Vec<u8>, WalletError> {
        todo!("Feature 1.3 — Kyber-1024 encryption")
    }

    /// Descifrar datos (ML-KEM-1024)
    pub fn decrypt(&self, _ciphertext: &[u8]) -> Result<Vec<u8>, WalletError> {
        todo!("Feature 1.3 — Kyber-1024 decryption")
    }

    /// Registrar un nodo en esta wallet
    ///
    /// El wallet firma (NodeID + WalletAddress) con Dilithium-5
    /// para probar que el nodo pertenece a este wallet.
    pub fn register_node(&mut self, _node_id: &str) -> Result<NodeBinding, WalletError> {
        todo!("Feature 1.3 — Register node to wallet")
    }

    /// Revocar un nodo de esta wallet
    pub fn revoke_node(&mut self, _node_id: &str) -> Result<(), WalletError> {
        todo!("Feature 1.3 — Revoke node from wallet")
    }

    /// Obtener el balance de $XAV
    pub fn balance(&self) -> u64 {
        self.state.as_ref().map(|w| w.balance).unwrap_or(0)
    }

    /// Mostrar estado de la wallet
    pub fn status(&self) -> WalletStatus {
        WalletStatus {
            address: self.state.as_ref().map(|w| w.address.clone()),
            balance: self.balance(),
            trust_score: self.state.as_ref().map(|w| w.trust_score).unwrap_or(0),
            contribution_score: self
                .state
                .as_ref()
                .map(|w| w.contribution_score)
                .unwrap_or(0),
            node_count: self.state.as_ref().map(|w| w.nodes.len()).unwrap_or(0),
            has_tpm: self.has_tpm,
        }
    }
}

#[derive(Debug)]
pub struct WalletStatus {
    pub address: Option<WalletAddress>,
    pub balance: u64,
    pub trust_score: i64,
    pub contribution_score: u64,
    pub node_count: usize,
    pub has_tpm: bool,
}

#[derive(Debug)]
pub enum WalletError {
    NoTpm,
    SeedGenerationFailed,
    KeyGenerationFailed,
    StorageFailed,
    InvalidSeed,
    WalletNotFound,
    WrongPassword,
    SignatureFailed,
    EncryptionFailed,
    NodeAlreadyRegistered,
    NodeNotRegistered,
    TpmError(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTpm => write!(f, "TPM 2.0 no disponible en este sistema"),
            Self::SeedGenerationFailed => write!(f, "Error generando seed phrase"),
            Self::KeyGenerationFailed => write!(f, "Error generando keypair post-cuántico"),
            Self::StorageFailed => write!(f, "Error almacenando wallet en disco"),
            Self::InvalidSeed => write!(f, "Seed phrase inválida o checksum incorrecto"),
            Self::WalletNotFound => write!(f, "No se encontró wallet en la ruta especificada"),
            Self::WrongPassword => write!(f, "Contraseña incorrecta"),
            Self::SignatureFailed => write!(f, "Error al firmar datos"),
            Self::EncryptionFailed => write!(f, "Error al cifrar/descifrar datos"),
            Self::NodeAlreadyRegistered => write!(f, "El nodo ya está registrado en esta wallet"),
            Self::NodeNotRegistered => write!(f, "El nodo no está registrado en esta wallet"),
            Self::TpmError(e) => write!(f, "Error de TPM: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO: Tests cuando la feature esté implementada
}
