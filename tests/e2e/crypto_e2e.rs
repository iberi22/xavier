use xavier::crypto::{
    decrypt_data, derive_kek_from_password, encrypt_data,
    encryption::{decrypt_with_session_key, encrypt_with_session_key, NonceBytes},
    generate_dek,
};

#[test]
fn test_client_side_envelope_encryption() {
    let password = "my_strong_agent_vault_password_2026";
    let salt = [0u8; 16]; // 16-byte salt for derivation

    // 1. Derive KEK (Key Encryption Key) from user password
    let kek =
        derive_kek_from_password(password, &salt).expect("Failed to derive KEK from password");

    // 2. Generate per-document DEK (Data Encryption Key)
    let dek = generate_dek();

    // 3. Encrypt the sensitive user content with the DEK
    let plaintext = b"Plaintext memory containing sensitive PII and keys.";
    let data_nonce = NonceBytes::generate();
    let encrypted_data_blob =
        encrypt_data(plaintext, &dek, &data_nonce).expect("Failed to encrypt data with DEK");

    // 4. Encrypt the DEK with the KEK (Key wrapping)
    let dek_nonce = NonceBytes::generate();
    let encrypted_dek_blob =
        encrypt_data(&dek, &kek.0, &dek_nonce).expect("Failed to encrypt DEK with KEK");

    // --- Decryption Flow ---

    // 5. Decrypt the wrapped DEK using KEK
    let decrypted_dek_bytes = decrypt_data(
        &encrypted_dek_blob.ciphertext,
        &kek.0,
        encrypted_dek_blob.nonce.as_slice().try_into().unwrap(),
    )
    .expect("Failed to decrypt DEK using KEK");

    let decrypted_dek: [u8; 32] = decrypted_dek_bytes.try_into().unwrap();

    // 6. Decrypt the original plaintext using the decrypted DEK
    let decrypted_plaintext = decrypt_data(
        &encrypted_data_blob.ciphertext,
        &decrypted_dek,
        encrypted_data_blob.nonce.as_slice().try_into().unwrap(),
    )
    .expect("Failed to decrypt data using DEK");

    assert_eq!(plaintext.to_vec(), decrypted_plaintext);
}

#[test]
fn test_node_session_key_encryption() {
    let original_message = b"Ephemeral node-to-node telemetry message";

    // 1. Encrypt with node session key
    let encrypted =
        encrypt_with_session_key(original_message).expect("Session key encryption failed");

    // 2. Decrypt with node session key
    let decrypted = decrypt_with_session_key(&encrypted).expect("Session key decryption failed");

    assert_eq!(original_message.to_vec(), decrypted);
}

#[test]
fn test_corrupted_ciphertext_verification() {
    let dek = generate_dek();
    let plaintext = b"Top secret agent instruction payload";
    let nonce = NonceBytes::generate();

    let mut blob = encrypt_data(plaintext, &dek, &nonce).expect("Encryption failed");

    // Attempt decryption with wrong key should fail
    let wrong_dek = generate_dek();
    let result_wrong_key = decrypt_data(&blob.ciphertext, &wrong_dek, nonce.as_bytes());
    assert!(result_wrong_key.is_err());

    // Tamper with the ciphertext (e.g., flip last byte of authenticated tag)
    if let Some(last) = blob.ciphertext.last_mut() {
        *last ^= 0x01;
    }

    // Decrypting tampered payload must fail
    let result_tampered = decrypt_data(&blob.ciphertext, &dek, nonce.as_bytes());
    assert!(result_tampered.is_err());
}
