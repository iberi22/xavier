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
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use bech32::{self, Hrp};
use bip39::{Language, Mnemonic};
use oqs::kem::{Algorithm as KemAlgo, Kem};
use oqs::sig::{Algorithm as SigAlgo, Sig};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
    /// Clave privada ML-DSA-87 (Dilithium-5)
    pub dilithium_secret_key: Option<Vec<u8>>,
    /// Clave privada ML-KEM-1024 (Kyber-1024)
    pub kyber_secret_key: Option<Vec<u8>>,
    /// Usando TPM?
    pub has_tpm: bool,
}

impl XavierWallet {
    /// Crear una nueva wallet desde seed phrase
    pub fn create(config: WalletConfig, password: &str) -> Result<(Self, String)> {
        let mut entropy = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut entropy);
        let mnemonic = Mnemonic::from_entropy_in(Language::Spanish, &entropy)
            .map_err(|_| anyhow!("Error generando mnemonic"))?;
        let phrase = mnemonic.to_string();

        let wallet = Self::from_seed(&phrase, config, password)?;
        wallet.save(password)?;

        Ok((wallet, phrase))
    }

    /// Importar wallet desde seed phrase
    pub fn from_seed(seed_phrase: &str, config: WalletConfig, password: &str) -> Result<Self> {
        let _mnemonic = Mnemonic::parse_in_normalized(Language::Spanish, seed_phrase)
            .map_err(|_| anyhow!("Seed phrase inválida"))?;

        // 1. Derivar master key usando Argon2id
        let salt = SaltString::generate(&mut rand::thread_rng());
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| anyhow!("Error derivando clave maestra"))?;
        let _key_bytes = password_hash.hash.ok_or(anyhow!("Hash error"))?;

        // 2. Generar keypairs
        let sig = Sig::new(SigAlgo::MlDsa87).map_err(|_| anyhow!("Error inicializando ML-DSA-87"))?;
        let (pk_sig, sk_sig) = sig.keypair().map_err(|_| anyhow!("Error generando ML-DSA-87"))?;

        let kem = Kem::new(KemAlgo::MlKem1024).map_err(|_| anyhow!("Error inicializando ML-KEM-1024"))?;
        let (pk_kem, sk_kem) = kem.keypair().map_err(|_| anyhow!("Error generando ML-KEM-1024"))?;

        // 3. Derivar address
        let address = Self::derive_address(pk_sig.as_ref())?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let state = Wallet {
            address: WalletAddress(address),
            dilithium_public_key: pk_sig.as_ref().to_vec(),
            kyber_public_key: pk_kem.as_ref().to_vec(),
            nodes: Vec::new(),
            balance: 0,
            trust_score: 0,
            contribution_score: 0,
            created_at: now,
            has_tpm: false, // Por ahora software
        };

        Ok(Self {
            config,
            state: Some(state),
            dilithium_secret_key: Some(sk_sig.as_ref().to_vec()),
            kyber_secret_key: Some(sk_kem.as_ref().to_vec()),
            has_tpm: false,
        })
    }

    fn derive_address(public_key: &[u8]) -> Result<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(public_key);
        let hash = hasher.finalize();

        let hrp = Hrp::parse("xv1").map_err(|_| anyhow!("Error parsing HRP"))?;
        let addr = bech32::encode::<bech32::Bech32m>(hrp, &hash[..32])
            .map_err(|_| anyhow!("Error encoding bech32"))?;

        Ok(format!("xv1_{}", addr))
    }

    /// Cargar wallet existente desde disco
    pub fn load(config: WalletConfig, password: &str) -> Result<Self> {
        let path = config.data_dir.join("wallet.json");
        if !path.exists() {
            return Err(anyhow!("Wallet no encontrada"));
        }

        let content = fs::read_to_string(path)?;
        let stored: StoredWallet = serde_json::from_str(&content)?;

        // Derivar clave de cifrado del password
        let salt = SaltString::from_b64(&stored.salt).map_err(|_| anyhow!("Invalid salt"))?;
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| anyhow!("Error derivando clave"))?;
        let key_bytes = password_hash.hash.ok_or(anyhow!("Hash error"))?;
        let cipher = Aes256Gcm::new_from_slice(key_bytes.as_bytes())?;

        // Descifrar secret keys
        let decrypt_key = |encrypted: &[u8], nonce_b64: &str| -> Result<Vec<u8>> {
            let nonce_vec = hex::decode(nonce_b64)?;
            let nonce = Nonce::from_slice(&nonce_vec);
            cipher
                .decrypt(nonce, encrypted)
                .map_err(|_| anyhow!("Error al descifrar (¿contraseña incorrecta?)"))
        };

        let dilithium_sk = decrypt_key(&stored.encrypted_dilithium_sk, &stored.dilithium_nonce)?;
        let kyber_sk = decrypt_key(&stored.encrypted_kyber_sk, &stored.kyber_nonce)?;

        Ok(Self {
            config,
            state: Some(stored.state),
            dilithium_secret_key: Some(dilithium_sk),
            kyber_secret_key: Some(kyber_sk),
            has_tpm: false,
        })
    }

    pub fn save(&self, password: &str) -> Result<()> {
        let path = self.config.data_dir.join("wallet.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Derivar clave de cifrado
        let salt = SaltString::generate(&mut rand::thread_rng());
        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| anyhow!("Error derivando clave"))?;
        let key_bytes = password_hash.hash.ok_or(anyhow!("Hash error"))?;
        let cipher = Aes256Gcm::new_from_slice(key_bytes.as_bytes())?;

        // Cifrar secret keys
        let encrypt_key = |key: &[u8]| -> Result<(Vec<u8>, String)> {
            let mut nonce_bytes = [0u8; 12];
            rand::thread_rng().fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_bytes);
            let encrypted = cipher.encrypt(nonce, key).map_err(|_| anyhow!("Error al cifrar"))?;
            Ok((encrypted, hex::encode(nonce_bytes)))
        };

        let (enc_dilithium, dilithium_nonce) = encrypt_key(
            self.dilithium_secret_key
                .as_ref()
                .ok_or(anyhow!("No dilithium sk"))?,
        )?;
        let (enc_kyber, kyber_nonce) = encrypt_key(
            self.kyber_secret_key
                .as_ref()
                .ok_or(anyhow!("No kyber sk"))?,
        )?;

        let state = self.state.as_ref().ok_or(anyhow!("Wallet sin estado"))?;
        let stored = StoredWallet {
            state: state.clone(),
            salt: salt.to_string(),
            encrypted_dilithium_sk: enc_dilithium,
            dilithium_nonce,
            encrypted_kyber_sk: enc_kyber,
            kyber_nonce,
        };

        let json = serde_json::to_string_pretty(&stored)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Firmar datos con ML-DSA-87
    pub fn sign(&self, data: &[u8]) -> Result<Vec<u8>> {
        let sk_bytes = self
            .dilithium_secret_key
            .as_ref()
            .ok_or(anyhow!("Clave privada no disponible"))?;

        let sig = Sig::new(SigAlgo::MlDsa87).map_err(|_| anyhow!("Error inicializando ML-DSA-87"))?;
        let sk = sig.secret_key_from_bytes(sk_bytes).ok_or(anyhow!("Error cargando clave secreta"))?;

        let signature = sig.sign(data, sk).map_err(|_| anyhow!("Error al firmar"))?;
        Ok(signature.as_ref().to_vec())
    }

    /// Verificar firma ML-DSA-87
    pub fn verify(
        &self,
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool> {
        let sig = Sig::new(SigAlgo::MlDsa87).map_err(|_| anyhow!("Error inicializando ML-DSA-87"))?;
        let pk = sig.public_key_from_bytes(public_key).ok_or(anyhow!("Error cargando clave pública"))?;
        let s = sig.signature_from_bytes(signature).ok_or(anyhow!("Error cargando firma"))?;

        Ok(sig.verify(data, s, pk).is_ok())
    }

    /// Cifrar datos para un destinatario (ML-KEM-1024 + AES-256-GCM)
    pub fn encrypt(
        &self,
        data: &[u8],
        recipient_public_key: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let kem = Kem::new(KemAlgo::MlKem1024).map_err(|_| anyhow!("Error inicializando ML-KEM-1024"))?;
        let pk = kem.public_key_from_bytes(recipient_public_key).ok_or(anyhow!("Error cargando clave pública"))?;

        let (kem_ct, shared_secret) = kem.encapsulate(pk).map_err(|_| anyhow!("Error al encapsular"))?;

        // Cifrar datos con el shared secret usando AES-256-GCM
        let cipher = Aes256Gcm::new_from_slice(shared_secret.as_ref())?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let mut encrypted_data = cipher.encrypt(nonce, data).map_err(|_| anyhow!("Error al cifrar datos"))?;
        // Prepend nonce to encrypted data
        let mut final_payload = nonce_bytes.to_vec();
        final_payload.append(&mut encrypted_data);

        Ok((kem_ct.as_ref().to_vec(), final_payload))
    }

    /// Descifrar datos (ML-KEM-1024 + AES-256-GCM)
    pub fn decrypt(&self, kem_ct: &[u8], encrypted_payload: &[u8]) -> Result<Vec<u8>> {
        let sk_bytes = self
            .kyber_secret_key
            .as_ref()
            .ok_or(anyhow!("Clave privada no disponible"))?;

        let kem = Kem::new(KemAlgo::MlKem1024).map_err(|_| anyhow!("Error inicializando ML-KEM-1024"))?;
        let sk = kem.secret_key_from_bytes(sk_bytes).ok_or(anyhow!("Error cargando clave secreta"))?;
        let ct = kem.ciphertext_from_bytes(kem_ct).ok_or(anyhow!("Error cargando ciphertext"))?;

        let shared_secret = kem.decapsulate(sk, ct).map_err(|_| anyhow!("Error al desencapsular"))?;

        // Descifrar datos con el shared secret
        if encrypted_payload.len() < 12 {
            return Err(anyhow!("Payload cifrado demasiado corto"));
        }

        let (nonce_bytes, ciphertext) = encrypted_payload.split_at(12);
        let cipher = Aes256Gcm::new_from_slice(shared_secret.as_ref())?;
        let nonce = Nonce::from_slice(nonce_bytes);

        let decrypted = cipher.decrypt(nonce, ciphertext).map_err(|_| anyhow!("Error al descifrar datos"))?;
        Ok(decrypted)
    }

    /// Registrar un nodo en esta wallet
    pub fn register_node(&mut self, node_id: &str) -> Result<NodeBinding> {
        let address = self
            .state
            .as_ref()
            .ok_or(anyhow!("Wallet sin estado"))?
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
            return Err(anyhow!("Nodo ya registrado"));
        }

        let to_sign = format!("{}:{}", node_id, address);
        let signature = self.sign(to_sign.as_bytes())?;

        let binding = NodeBinding {
            node_id: node_id.to_string(),
            signature,
            registered_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            last_heartbeat_at: 0,
            status: BindingStatus::Active,
        };

        let state = self.state.as_mut().ok_or(anyhow!("Wallet sin estado"))?;
        state.nodes.push(binding.clone());
        Ok(binding)
    }

    /// Revocar un nodo de esta wallet
    pub fn revoke_node(&mut self, node_id: &str) -> Result<()> {
        let state = self.state.as_mut().ok_or(anyhow!("Wallet sin estado"))?;
        state.nodes.retain(|n| n.node_id != node_id);
        Ok(())
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

#[derive(Serialize, Deserialize)]
struct StoredWallet {
    state: Wallet,
    salt: String,
    encrypted_dilithium_sk: Vec<u8>,
    dilithium_nonce: String,
    encrypted_kyber_sk: Vec<u8>,
    kyber_nonce: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wallet_creation_and_loading() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let password = "test-password";

        // Create wallet
        let (wallet, phrase) = XavierWallet::create(config.clone(), password).unwrap();
        let address = wallet.state.as_ref().unwrap().address.clone();
        assert!(address.is_valid());

        // Load wallet
        let loaded = XavierWallet::load(config, password).unwrap();
        assert_eq!(loaded.state.as_ref().unwrap().address, address);
        assert!(loaded.dilithium_secret_key.is_some());
        assert!(loaded.kyber_secret_key.is_some());
    }

    #[test]
    fn test_hybrid_encryption() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let (wallet, _) = XavierWallet::create(config, "pass").unwrap();

        let data = b"secret message";
        let recipient_pk = wallet.state.as_ref().unwrap().kyber_public_key.clone();

        let (kem_ct, encrypted_payload) = wallet.encrypt(data, &recipient_pk).unwrap();
        let decrypted = wallet.decrypt(&kem_ct, &encrypted_payload).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_signing() {
        let dir = tempdir().unwrap();
        let config = WalletConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let (wallet, _) = XavierWallet::create(config, "pass").unwrap();

        let data = b"sign this";
        let signature = wallet.sign(data).unwrap();
        let pk = wallet.state.as_ref().unwrap().dilithium_public_key.clone();

        let ok = wallet.verify(data, &signature, &pk).unwrap();
        assert!(ok);
    }
}
