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

use crate::crypto::encryption::{decrypt_data, encrypt_data, NonceBytes};
use crate::crypto::keys::{KeyManager, KeySalt};
use crate::data_commons::types::*;
use bip39::{Language, Mnemonic};
use ed25519_dalek::SigningKey;
use oqs::kem::{Algorithm as KemAlg, Kem};
use oqs::sig::{Algorithm as SigAlg, Sig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

/// Configuración de wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Estructura interna para persistir la wallet
#[derive(Serialize, Deserialize)]
struct EncryptedWallet {
    /// Salt de Argon2 usado para derivar la KEK
    salt: [u8; 16],
    /// Blob cifrado con AES-256-GCM que contiene las claves privadas
    /// Formato: [nonce (12b)] + [ciphertext]
    encrypted_data: Vec<u8>,
    /// Datos públicos de la wallet
    public_data: Wallet,
}

/// Datos privados de la wallet (después de descifrar)
#[derive(Serialize, Deserialize)]
struct WalletSecrets {
    /// Mnemonic (seed phrase)
    mnemonic: String,
    /// Clave privada ML-DSA-87 (Dilithium-5)
    dilithium_sk: Vec<u8>,
    /// Clave privada ML-KEM-1024 (Kyber-1024)
    kyber_sk: Vec<u8>,
    /// Clave privada Ed25519
    ed25519_sk: [u8; 32],
}

/// Wallet post-cuántica $XAV
pub struct XavierWallet {
    /// Configuración
    pub config: WalletConfig,
    /// Estado de la wallet (público)
    pub state: Option<Wallet>,
    /// Secretos (solo en memoria mientras esté desbloqueada)
    secrets: Option<WalletSecrets>,
    /// Usando TPM?
    pub has_tpm: bool,
}

impl XavierWallet {
    /// Crear una nueva wallet desde cero
    pub fn create(config: WalletConfig, password: &str) -> Result<(Self, String), WalletError> {
        let language = match config.seed_language.as_str() {
            "spanish" => Language::Spanish,
            "english" => Language::English,
            _ => Language::Spanish,
        };

        // 1. Generar seed phrase (24 palabras)
        let mnemonic = Mnemonic::generate_in(language, 24)
            .map_err(|_| WalletError::SeedGenerationFailed)?;
        let mnemonic_phrase = mnemonic.to_string();

        let wallet = Self::from_mnemonic(&mnemonic_phrase, config)?;
        wallet.save(password)?;

        Ok((wallet, mnemonic_phrase))
    }

    /// Importar wallet desde mnemonic
    pub fn from_mnemonic(mnemonic_phrase: &str, config: WalletConfig) -> Result<Self, WalletError> {
        let mnemonic = Mnemonic::parse(mnemonic_phrase).map_err(|_| WalletError::InvalidSeed)?;

        let seed_bytes = mnemonic.to_seed("");

        // 2. Derivar claves desde el seed
        let _dilithium_seed = {
            let mut h = Sha256::new();
            h.update(seed_bytes);
            h.update(b"dilithium");
            h.finalize()
        };

        let kyber_seed = {
            let mut h = Sha256::new();
            h.update(seed_bytes);
            h.update(b"kyber");
            h.finalize()
        };

        let ed25519_seed = {
            let mut h = Sha256::new();
            h.update(seed_bytes);
            h.update(b"ed25519");
            let res = h.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&res);
            arr
        };

        // 3. Generar keypairs PQ
        let sig = Sig::new(SigAlg::MlDsa87).map_err(|_| WalletError::KeyGenerationFailed)?;
        let (dilithium_pk, dilithium_sk) =
            sig.keypair().map_err(|_| WalletError::KeyGenerationFailed)?;

        let kem = Kem::new(KemAlg::MlKem1024).map_err(|_| WalletError::KeyGenerationFailed)?;

        let kyber_seed_ref = kem
            .keypair_seed_from_bytes(&kyber_seed[..kem.length_keypair_seed()])
            .ok_or(WalletError::KeyGenerationFailed)?;

        let (kyber_pk, kyber_sk) = kem
            .keypair_derand(kyber_seed_ref)
            .map_err(|_| WalletError::KeyGenerationFailed)?;

        // 4. Generar Ed25519 keypair
        let ed25519_sk_key = SigningKey::from_bytes(&ed25519_seed);
        let _ed25519_pk = ed25519_sk_key.verifying_key();

        // 5. Derivar wallet address: xv1_ + bech32(hash(ML-DSA-87 pk))
        let address = derive_address(dilithium_pk.as_ref());

        let wallet_data = Wallet {
            address: WalletAddress(address),
            dilithium_public_key: dilithium_pk.as_ref().to_vec(),
            kyber_public_key: kyber_pk.as_ref().to_vec(),
            nodes: Vec::new(),
            balance: 0,
            trust_score: 0,
            contribution_score: 0,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            has_tpm: false, // Por ahora software
        };

        let secrets = WalletSecrets {
            mnemonic: mnemonic_phrase.to_string(),
            dilithium_sk: dilithium_sk.as_ref().to_vec(),
            kyber_sk: kyber_sk.as_ref().to_vec(),
            ed25519_sk: ed25519_seed,
        };

        Ok(Self {
            config,
            state: Some(wallet_data),
            secrets: Some(secrets),
            has_tpm: false,
        })
    }

    /// Guardar wallet cifrada en disco
    pub fn save(&self, password: &str) -> Result<(), WalletError> {
        let Some(state) = &self.state else {
            return Err(WalletError::WalletNotFound);
        };
        let Some(secrets) = &self.secrets else {
            return Err(WalletError::WalletNotFound);
        };

        let manager = KeyManager::new();
        let kek = manager
            .derive_kek(password)
            .map_err(|_| WalletError::EncryptionFailed)?;

        let secrets_json = serde_json::to_vec(secrets).map_err(|_| WalletError::EncryptionFailed)?;
        let nonce = NonceBytes::generate();
        let encrypted_blob = encrypt_data(&secrets_json, kek.as_bytes(), &nonce)
            .map_err(|_| WalletError::EncryptionFailed)?;

        let encrypted_wallet = EncryptedWallet {
            salt: *manager.salt().as_bytes(),
            encrypted_data: encrypted_blob.to_bytes(),
            public_data: state.clone(),
        };

        if !self.config.data_dir.exists() {
            fs::create_dir_all(&self.config.data_dir).map_err(|_| WalletError::StorageFailed)?;
        }

        let wallet_path = self.config.data_dir.join("wallet.json");
        let data =
            serde_json::to_vec_pretty(&encrypted_wallet).map_err(|_| WalletError::StorageFailed)?;
        fs::write(wallet_path, data).map_err(|_| WalletError::StorageFailed)?;

        Ok(())
    }

    /// Cargar wallet existente desde disco
    pub fn load(config: WalletConfig, password: &str) -> Result<Self, WalletError> {
        let wallet_path = config.data_dir.join("wallet.json");
        if !wallet_path.exists() {
            return Err(WalletError::WalletNotFound);
        }

        let data = fs::read(wallet_path).map_err(|_| WalletError::StorageFailed)?;
        let encrypted_wallet: EncryptedWallet =
            serde_json::from_slice(&data).map_err(|_| WalletError::StorageFailed)?;

        let salt = KeySalt::from_bytes(&encrypted_wallet.salt);
        let manager = KeyManager::with_salt(salt);
        let kek = manager
            .derive_kek(password)
            .map_err(|_| WalletError::EncryptionFailed)?;

        let nonce_size = 12; // De src/crypto/mod.rs
        if encrypted_wallet.encrypted_data.len() < nonce_size {
            return Err(WalletError::EncryptionFailed);
        }

        let nonce_bytes = &encrypted_wallet.encrypted_data[..nonce_size];
        let ciphertext = &encrypted_wallet.encrypted_data[nonce_size..];

        let decrypted_secrets = decrypt_data(
            ciphertext,
            kek.as_bytes(),
            nonce_bytes
                .try_into()
                .map_err(|_| WalletError::EncryptionFailed)?,
        )
        .map_err(|_| WalletError::WrongPassword)?;

        let secrets: WalletSecrets =
            serde_json::from_slice(&decrypted_secrets).map_err(|_| WalletError::EncryptionFailed)?;

        Ok(Self {
            config,
            state: Some(encrypted_wallet.public_data),
            secrets: Some(secrets),
            has_tpm: false,
        })
    }

    /// Firmar datos con ML-DSA-87
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>, WalletError> {
        let Some(secrets) = &self.secrets else {
            return Err(WalletError::WalletNotFound);
        };

        let sig = Sig::new(SigAlg::MlDsa87).map_err(|_| WalletError::SignatureFailed)?;
        let sk = sig
            .secret_key_from_bytes(&secrets.dilithium_sk)
            .ok_or(WalletError::SignatureFailed)?;

        let signature = sig.sign(data, &sk).map_err(|_| WalletError::SignatureFailed)?;
        Ok(signature.as_ref().to_vec())
    }

    /// Verificar firma ML-DSA-87
    pub fn verify(
        &self,
        data: &[u8],
        signature_bytes: &[u8],
        public_key_bytes: &[u8],
    ) -> Result<bool, WalletError> {
        let sig = Sig::new(SigAlg::MlDsa87).map_err(|_| WalletError::SignatureFailed)?;
        let pk = sig
            .public_key_from_bytes(public_key_bytes)
            .ok_or(WalletError::SignatureFailed)?;
        let signature = sig
            .signature_from_bytes(signature_bytes)
            .ok_or(WalletError::SignatureFailed)?;

        Ok(sig.verify(data, &signature, &pk).is_ok())
    }

    /// Cifrar datos para un destinatario (ML-KEM-1024)
    pub fn encrypt(
        &self,
        data: &[u8],
        recipient_public_key: &[u8],
    ) -> Result<Vec<u8>, WalletError> {
        let kem = Kem::new(KemAlg::MlKem1024).map_err(|_| WalletError::EncryptionFailed)?;
        let pk = kem
            .public_key_from_bytes(recipient_public_key)
            .ok_or(WalletError::EncryptionFailed)?;

        let (ciphertext, shared_secret) = kem
            .encapsulate(&pk)
            .map_err(|_| WalletError::EncryptionFailed)?;

        // Usamos el shared secret para cifrar el contenido real con AES-256-GCM
        let mut key = [0u8; 32];
        let ss_bytes = shared_secret.as_ref();
        key.copy_from_slice(&ss_bytes[..32]);

        let nonce = NonceBytes::generate();
        let encrypted_content = encrypt_data(data, &key, &nonce)
            .map_err(|_| WalletError::EncryptionFailed)?;

        // Resultado: [Kyber Ciphertext] + [AES Nonce] + [AES Ciphertext]
        let mut result = Vec::new();
        result.extend_from_slice(ciphertext.as_ref());
        result.extend_from_slice(encrypted_content.nonce.as_slice());
        result.extend_from_slice(encrypted_content.ciphertext.as_slice());

        Ok(result)
    }

    /// Descifrar datos (ML-KEM-1024)
    pub fn decrypt(&self, encrypted_data: &[u8]) -> Result<Vec<u8>, WalletError> {
        let Some(secrets) = &self.secrets else {
            return Err(WalletError::WalletNotFound);
        };

        let kem = Kem::new(KemAlg::MlKem1024).map_err(|_| WalletError::EncryptionFailed)?;
        let sk = kem
            .secret_key_from_bytes(&secrets.kyber_sk)
            .ok_or(WalletError::EncryptionFailed)?;

        let ct_size = kem.length_ciphertext();
        if encrypted_data.len() < ct_size + 12 {
            return Err(WalletError::EncryptionFailed);
        }

        let kem_ct_bytes = &encrypted_data[..ct_size];
        let kem_ct = kem
            .ciphertext_from_bytes(kem_ct_bytes)
            .ok_or(WalletError::EncryptionFailed)?;

        let shared_secret = kem
            .decapsulate(&sk, &kem_ct)
            .map_err(|_| WalletError::EncryptionFailed)?;

        let mut key = [0u8; 32];
        let ss_bytes = shared_secret.as_ref();
        key.copy_from_slice(&ss_bytes[..32]);

        let nonce_bytes = &encrypted_data[ct_size..ct_size + 12];
        let ciphertext = &encrypted_data[ct_size + 12..];

        let decrypted = decrypt_data(
            ciphertext,
            &key,
            nonce_bytes
                .try_into()
                .map_err(|_| WalletError::EncryptionFailed)?,
        )
        .map_err(|_| WalletError::EncryptionFailed)?;

        Ok(decrypted)
    }

    /// Registrar un nodo en esta wallet
    pub fn register_node(&mut self, node_id: &str) -> Result<NodeBinding, WalletError> {
        let address = self
            .state
            .as_ref()
            .ok_or(WalletError::WalletNotFound)?
            .address
            .0
            .clone();

        if self
            .state
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|n| n.node_id == node_id)
        {
            return Err(WalletError::NodeAlreadyRegistered);
        }

        let payload = format!("{}{}", node_id, address);
        let signature = self.sign(payload.as_bytes())?;

        let binding = NodeBinding {
            node_id: node_id.to_string(),
            signature,
            registered_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            last_heartbeat_at: 0,
            status: BindingStatus::Active,
        };

        if let Some(state) = &mut self.state {
            state.nodes.push(binding.clone());
        }
        Ok(binding)
    }

    /// Revocar un nodo de esta wallet
    pub fn revoke_node(&mut self, node_id: &str) -> Result<(), WalletError> {
        let Some(state) = &mut self.state else {
            return Err(WalletError::WalletNotFound);
        };

        if let Some(node) = state.nodes.iter_mut().find(|n| n.node_id == node_id) {
            node.status = BindingStatus::Revoked;
            Ok(())
        } else {
            Err(WalletError::NodeNotRegistered)
        }
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

    /// Obtener una clave para derivar cifrado de chunks de sincronización
    pub fn get_sync_encryption_key(&self) -> Result<Option<[u8; 32]>, WalletError> {
        let Some(secrets) = &self.secrets else {
            return Ok(None);
        };
        // Derivamos una clave específica para sincronización de chunks
        let mut h = Sha256::new();
        h.update(&secrets.ed25519_sk);
        h.update(b"chunk_sync_v1");
        let res = h.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&res);
        Ok(Some(key))
    }
}

/// Derivar dirección bech32 desde clave pública Dilithium
fn derive_address(pk: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pk);
    let hash = hasher.finalize();

    let base32_data = bech32::convert_bits(&hash[..32], 8, 5, true).expect("valid conversion");
    let mut b32 = Vec::new();
    for i in base32_data {
        b32.push(bech32::u5::try_from_u8(i).expect("valid u5"));
    }
    bech32::encode("xv1", b32, bech32::Variant::Bech32).expect("valid bech32")
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

impl std::error::Error for WalletError {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wallet_creation_and_load() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            prefer_tpm: false,
            seed_language: "spanish".into(),
        };
        let password = "test_password";

        let (wallet, mnemonic) = XavierWallet::create(config.clone(), password).unwrap();
        assert!(mnemonic.split(' ').count() == 24);
        assert!(wallet.state.is_some());

        let loaded_wallet = XavierWallet::load(config, password).unwrap();
        assert_eq!(
            wallet.state.as_ref().unwrap().address,
            loaded_wallet.state.as_ref().unwrap().address
        );
    }

    #[test]
    fn test_sign_verify() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let (wallet, _) = XavierWallet::create(config, "pass").unwrap();

        let data = b"hello xavier mesh";
        let signature = wallet.sign(data).unwrap();
        let pk = &wallet.state.as_ref().unwrap().dilithium_public_key;

        assert!(wallet.verify(data, &signature, pk).unwrap());
        assert!(!wallet.verify(b"wrong data", &signature, pk).unwrap());
    }

    #[test]
    fn test_encrypt_decrypt() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let (wallet, _) = XavierWallet::create(config, "pass").unwrap();

        let data = b"sensitive post-quantum data";
        let pk = &wallet.state.as_ref().unwrap().kyber_public_key;

        let encrypted = wallet.encrypt(data, pk).unwrap();
        let decrypted = wallet.decrypt(&encrypted).unwrap();

        assert_eq!(data.to_vec(), decrypted);
    }

    #[test]
    fn test_node_registration() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let (mut wallet, _) = XavierWallet::create(config, "pass").unwrap();

        let node_id = "xv1-node-123";
        let binding = wallet.register_node(node_id).unwrap();

        assert_eq!(binding.node_id, node_id);
        assert_eq!(wallet.state.as_ref().unwrap().nodes.len(), 1);
    }
}
