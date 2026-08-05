use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use rand::rngs::OsRng;
use std::env;
use tracing::warn;
use x25519_dalek::{PublicKey, StaticSecret};

/// Obtiene la llave privada estática del nodo mantenedor.
/// En un escenario de producción, esta llave X25519 privada se cargaría
/// desde un Secure Enclave o KMS. Fallará de forma segura si la variable de entorno
/// no está configurada, evitando fallbacks hardcodeados.
pub fn get_maintainer_secret() -> anyhow::Result<StaticSecret> {
    let hex_val = env::var("XAVIER_MAINTAINER_PRIVATE_KEY_HEX")
        .map_err(|_| anyhow::anyhow!("XAVIER_MAINTAINER_PRIVATE_KEY_HEX is missing"))?;

    let mut bytes = [0u8; 32];
    let decoded = crate::crypto::hex_decode(&hex_val)
        .map_err(|_| anyhow::anyhow!("XAVIER_MAINTAINER_PRIVATE_KEY_HEX is not valid hex"))?;

    if decoded.len() == 32 {
        bytes.copy_from_slice(&decoded);
        Ok(StaticSecret::from(bytes))
    } else {
        anyhow::bail!("XAVIER_MAINTAINER_PRIVATE_KEY_HEX must be exactly 32 bytes");
    }
}

/// Obtiene la llave pública del nodo mantenedor.
/// Todos los nodos en la red tienen acceso a esta función para cifrar la telemetría.
pub fn get_maintainer_public_key() -> anyhow::Result<PublicKey> {
    Ok(PublicKey::from(&get_maintainer_secret()?))
}

/// Verifica si este nodo está configurado como nodo mantenedor.
pub fn is_maintainer_node() -> bool {
    env::var("XAVIER_IS_MAINTAINER").unwrap_or_else(|_| "true".to_string()) == "true"
}

/// Cifra un payload usando Cifrado Asimétrico Híbrido (ECIES).
/// 1. Genera un par de llaves efímero.
/// 2. Deriva el secreto compartido usando ECDH.
/// 3. Usa el secreto compartido como llave AES-256-GCM.
/// 4. Retorna el (payload_cifrado, pubkey_efimera).
pub fn encrypt_for_maintainer(
    payload_json: &str,
) -> Result<(Vec<u8>, [u8; 32]), crate::crypto::encryption::EncryptionError> {
    let maintainer_pub = get_maintainer_public_key()
        .map_err(|_| crate::crypto::encryption::EncryptionError::InvalidKey)?;

    // 1. Generar llave efímera para esta única transacción
    let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);

    // 2. Derivar llave simétrica compartida
    let shared_secret = ephemeral_secret.diffie_hellman(&maintainer_pub);

    // 3. Cifrar el payload usando AES-256-GCM y la llave compartida
    let nonce = NonceBytes::generate();
    let encrypted_payload = aes_encrypt(payload_json.as_bytes(), shared_secret.as_bytes(), &nonce)?;

    Ok((encrypted_payload, ephemeral_public.to_bytes()))
}

/// Descifra un payload cifrado con Cifrado Asimétrico Híbrido.
/// Exclusivo para el Nodo Mantenedor.
pub fn decrypt_as_maintainer(
    encrypted_payload: &[u8],
    ephemeral_pubkey_bytes: &[u8; 32],
) -> Result<String, crate::crypto::encryption::EncryptionError> {
    if !is_maintainer_node() {
        warn!("Intento de descifrado en un nodo que no es mantenedor.");
    }

    let maintainer_secret = get_maintainer_secret()
        .map_err(|_| crate::crypto::encryption::EncryptionError::InvalidKey)?;
    let ephemeral_pub = PublicKey::from(*ephemeral_pubkey_bytes);

    // Reconstruir la misma llave compartida
    let shared_secret = maintainer_secret.diffie_hellman(&ephemeral_pub);

    // Descifrar
    let decrypted_bytes = aes_decrypt(encrypted_payload, shared_secret.as_bytes())?;

    Ok(String::from_utf8_lossy(&decrypted_bytes).to_string())
}
