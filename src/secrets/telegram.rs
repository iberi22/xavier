use crate::crypto::{encryption, keys};
use crate::crypto::keys::KeySalt;
use crate::secrets::{SecretError, SecretResult};
use std::fs;
use std::path::PathBuf;

pub struct TelegramBotTokenManager {
    storage_path: PathBuf,
}

impl TelegramBotTokenManager {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let storage_path = home.join(".xavier").join("telegram.enc");
        Self { storage_path }
    }

    fn get_master_key() -> SecretResult<String> {
        std::env::var("XAVIER_TOKEN")
            .or_else(|_| std::env::var("XAVIER_TOKEN_SECRET"))
            .map_err(|_| SecretError::ProviderError("XAVIER_TOKEN or XAVIER_TOKEN_SECRET must be set for encryption".to_string()))
    }

    pub fn store_token(&self, token: &str) -> SecretResult<()> {
        let master_key = Self::get_master_key()?;
        let salt = KeySalt::generate();
        let kek = keys::KEK::derive_from_password(&master_key, &salt)
            .map_err(|e| SecretError::ProviderError(e.to_string()))?;

        let nonce = encryption::NonceBytes::generate();
        let blob = encryption::encrypt_data(token.as_bytes(), kek.as_bytes(), &nonce)
            .map_err(|e| SecretError::ProviderError(e.to_string()))?;

        let mut data = Vec::new();
        data.extend_from_slice(salt.as_bytes());
        data.extend_from_slice(&blob.to_bytes());

        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent).map_err(|e| SecretError::DatabaseError(e.to_string()))?;
        }
        fs::write(&self.storage_path, data).map_err(|e| SecretError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub fn get_token(&self) -> SecretResult<Option<String>> {
        if !self.storage_path.exists() {
            return Ok(None);
        }

        let data = fs::read(&self.storage_path).map_err(|e| SecretError::DatabaseError(e.to_string()))?;
        if data.len() < crate::crypto::SALT_SIZE + crate::crypto::NONCE_SIZE {
            return Err(SecretError::ProviderError("Stored token data is corrupted".to_string()));
        }

        let (salt_bytes, blob_bytes) = data.split_at(crate::crypto::SALT_SIZE);
        let salt = KeySalt::from_bytes(salt_bytes.try_into().unwrap());

        let master_key = Self::get_master_key()?;
        let kek = keys::KEK::derive_from_password(&master_key, &salt)
            .map_err(|e| SecretError::ProviderError(e.to_string()))?;

        let blob = encryption::EncryptedBlob::from_bytes(blob_bytes)
            .map_err(|e| SecretError::ProviderError(e.to_string()))?;

        let nonce: [u8; crate::crypto::NONCE_SIZE] = blob.nonce.clone().try_into()
            .map_err(|_| SecretError::ProviderError("Invalid nonce".to_string()))?;

        let decrypted = encryption::decrypt_data(&blob.ciphertext, kek.as_bytes(), &nonce)
            .map_err(|e| SecretError::ProviderError(e.to_string()))?;

        Ok(Some(String::from_utf8(decrypted).map_err(|e| SecretError::ProviderError(e.to_string()))?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_token_encrypt_decrypt_roundtrip() {
        std::env::set_var("XAVIER_TOKEN", "test-master-token");
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_path = temp_dir.path().join("telegram.enc");
        let manager = TelegramBotTokenManager { storage_path };

        let token = "123456789:ABCdefGHIjklMNOpqrSTUvwxYZ";
        manager.store_token(token).unwrap();

        let retrieved = manager.get_token().unwrap().unwrap();
        assert_eq!(token, retrieved);
    }

    #[test]
    fn test_telegram_token_unconfigured_returns_none() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_path = temp_dir.path().join("nonexistent.enc");
        let manager = TelegramBotTokenManager { storage_path };
        assert!(manager.get_token().unwrap().is_none());
    }
}
