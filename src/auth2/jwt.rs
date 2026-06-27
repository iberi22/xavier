use jsonwebtoken::{encode, decode, Header, Algorithm, Validation, EncodingKey, DecodingKey};
use rsa::{RsaPrivateKey, RsaPublicKey, pkcs8::{EncodePrivateKey, EncodePublicKey, DecodePrivateKey, DecodePublicKey}};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::{anyhow, Result};
use crate::secrets::vault::HardwareVault;
use crate::crypto::encryption::{encrypt_data, decrypt_data, NonceBytes, EncryptedBlob};
use crate::crypto::NONCE_SIZE;
use rand::rngs::OsRng;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub role: String,
    pub iat: u64,
    pub exp: u64,
}

pub struct JwtManager {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtManager {
    pub fn new() -> Result<Self> {
        let (private_key_pem, public_key_pem) = Self::get_or_create_keypair()?;

        let encoding_key = EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
            .map_err(|e| anyhow!("Failed to create encoding key: {}", e))?;
        let decoding_key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
            .map_err(|e| anyhow!("Failed to create decoding key: {}", e))?;

        Ok(Self { encoding_key, decoding_key })
    }

    fn get_or_create_keypair() -> Result<(String, String)> {
        let vault = HardwareVault::new("xavier-auth");

        // Try to get from vault
        if let (Ok(enc_private), Ok(enc_public)) = (vault.get_secret("JWT_PRIVATE_KEY"), vault.get_secret("JWT_PUBLIC_KEY")) {
            // Decrypt keys
            let master_key = HardwareVault::new("xavier-auth").get_secret("DB_MASTER_KEY")?;
            let mut key_bytes = [0u8; 32];
            let decoded_master = crate::crypto::hex_decode(&master_key)?;
            key_bytes.copy_from_slice(&decoded_master[..32]);

            let private_blob = EncryptedBlob::from_bytes(&crate::crypto::hex_decode(&enc_private)?)
                .map_err(|_| anyhow!("Invalid private key blob"))?;
            let public_blob = EncryptedBlob::from_bytes(&crate::crypto::hex_decode(&enc_public)?)
                .map_err(|_| anyhow!("Invalid public key blob"))?;

            let private_pem = String::from_utf8(decrypt_data(&private_blob.ciphertext, &key_bytes, &private_blob.nonce.try_into().unwrap())?)?;
            let public_pem = String::from_utf8(decrypt_data(&public_blob.ciphertext, &key_bytes, &public_blob.nonce.try_into().unwrap())?)?;

            return Ok((private_pem, public_pem));
        }

        // Generate new keypair
        let mut rng = OsRng;
        let bits = 2048;
        let priv_key = RsaPrivateKey::new(&mut rng, bits)
            .map_err(|e| anyhow!("Failed to generate RSA key: {}", e))?;
        let pub_key = RsaPublicKey::from(&priv_key);

        let private_pem = priv_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|e| anyhow!("Failed to encode private key: {}", e))?
            .to_string();
        let public_pem = pub_key.to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .map_err(|e| anyhow!("Failed to encode public key: {}", e))?;

        // Encrypt and store in vault
        let master_key = HardwareVault::new("xavier-auth").get_secret("DB_MASTER_KEY")?;
        let mut key_bytes = [0u8; 32];
        let decoded_master = crate::crypto::hex_decode(&master_key)?;
        key_bytes.copy_from_slice(&decoded_master[..32]);

        let private_nonce = NonceBytes::generate();
        let public_nonce = NonceBytes::generate();

        let enc_private = encrypt_data(private_pem.as_bytes(), &key_bytes, &private_nonce)?;
        let enc_public = encrypt_data(public_pem.as_bytes(), &key_bytes, &public_nonce)?;

        vault.store_secret("JWT_PRIVATE_KEY", &crate::crypto::hex_encode(enc_private.to_bytes()))?;
        vault.store_secret("JWT_PUBLIC_KEY", &crate::crypto::hex_encode(enc_public.to_bytes()))?;

        Ok((private_pem, public_pem))
    }

    pub fn create_token(&self, user_id: &str, email: &str, role: &str) -> Result<String> {
        let iat = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let exp = iat + 15 * 60; // 15 minutes

        let claims = Claims {
            sub: user_id.to_string(),
            email: email.to_string(),
            role: role.to_string(),
            iat,
            exp,
        };

        let header = Header::new(Algorithm::RS256);
        encode(&header, &claims, &self.encoding_key)
            .map_err(|e| anyhow!("Failed to create token: {}", e))
    }

    pub fn validate_token(&self, token: &str) -> Result<Claims> {
        let validation = Validation::new(Algorithm::RS256);
        let token_data = decode::<Claims>(token, &self.decoding_key, &validation)
            .map_err(|e| anyhow!("Invalid token: {}", e))?;

        Ok(token_data.claims)
    }
}
