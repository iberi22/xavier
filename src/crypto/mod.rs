//! Crypto Module - E2E Encryption for Xavier Cloud Tier
//!
//! This module provides end-to-end encryption for the cloud storage tier.
//! The server NEVER sees plaintext or keys - all encryption/decryption happens client-side.
//!
//! # Architecture
//!
//! ```text
//! User Data → Encrypted on Client → Stored Encrypted in Cloud → Never Decrypted by Server
//! ```
//!
//! # Key Hierarchy
//!
//! - **KEK (Key Encryption Key)**: Derived from user password via Argon2id
//! - **DEK (Data Encryption Key)**: Per-document random key, encrypted with KEK
//!
//! # Encryption Flow
//!
//! ```text
//! User Password → Argon2id → KEK
//! DEK = GenerateRandomKey(32 bytes)
//! Encrypted_DEK = AES-256-GCM(DEK, KEK, iv_kek)
//! Encrypted_Data = AES-256-GCM(plaintext, DEK, iv_data)
//! Store: Encrypted_DEK + Encrypted_Data + iv_kek + iv_data + salt
//! ```

pub mod encryption;
pub mod envelope;
pub mod hmac;
pub mod keys;
pub mod password;
pub mod wallet;

pub use encryption::{decrypt_data, encrypt_data, EncryptedBlob};
pub use keys::{derive_kek_from_password, generate_dek, KeyManager};
pub use wallet::{Ed25519Wallet, WalletError, WalletResult};

/// Size of DEK (Data Encryption Key) in bytes
pub const DEK_SIZE: usize = 32;

/// Size of salt for Argon2 in bytes
pub const SALT_SIZE: usize = 16;

/// Size of nonce/IV for AES-256-GCM in bytes
pub const NONCE_SIZE: usize = 12;

// ---------------------------------------------------------------------------
// Inline hex encoding/decoding (replaces `hex` crate)
// ---------------------------------------------------------------------------

const HEX_CHARS: &[u8] = b"0123456789abcdef";

/// Encode bytes as lowercase hex string.
pub fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let hex_str = bytes
        .iter()
        .flat_map(|b| [HEX_CHARS[(b >> 4) as usize], HEX_CHARS[(b & 0x0f) as usize]])
        .collect::<Vec<_>>();
    String::from_utf8(hex_str).unwrap_or_default()
}

/// Decode a hex string into bytes. Returns an error on invalid input.
pub fn hex_decode(hex_str: &str) -> anyhow::Result<Vec<u8>> {
    if !hex_str.len().is_multiple_of(2) {
        anyhow::bail!("hex string must have even length");
    }
    let bytes = hex_str.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let hi = hex_val(chunk[0])
            .ok_or_else(|| anyhow::anyhow!("invalid hex char '{}'", chunk[0] as char))?;
        let lo = hex_val(chunk[1])
            .ok_or_else(|| anyhow::anyhow!("invalid hex char '{}'", chunk[1] as char))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Inline base64 encoding/decoding (RFC 4648, replaces `base64` crate)
// ---------------------------------------------------------------------------

const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const B64_PAD: u8 = b'=';

/// Encode bytes as base64 (standard, with padding).
pub fn base64_encode(data: impl AsRef<[u8]>) -> String {
    let data = data.as_ref();
    if data.is_empty() {
        return String::new();
    }
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        result.push(B64_CHARS[((b0 >> 2) & 0x3f) as usize] as char);
        result.push(B64_CHARS[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
        result.push(if chunk.len() > 1 {
            B64_CHARS[(((b1 << 2) | (b2 >> 6)) & 0x3f) as usize] as char
        } else {
            B64_PAD as char
        });
        result.push(if chunk.len() > 2 {
            B64_CHARS[(b2 & 0x3f) as usize] as char
        } else {
            B64_PAD as char
        });
    }
    result
}

/// Decode a base64 string (standard, with or without padding).
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let input = input.trim_end_matches('=');
    if input.is_empty() {
        return Some(Vec::new());
    }
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity((bytes.len() / 4) * 3);
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let mut buf = [0u8; 4];
        for i in 0..chunk.len() {
            buf[i] = match chunk[i] {
                b'A'..=b'Z' => chunk[i] - b'A',
                b'a'..=b'z' => chunk[i] - b'a' + 26,
                b'0'..=b'9' => chunk[i] - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => return None,
            };
        }
        if chunk.len() == 4 {
            let triple = ((buf[0] as u32) << 18)
                | ((buf[1] as u32) << 12)
                | ((buf[2] as u32) << 6)
                | buf[3] as u32;
            result.push((triple >> 16) as u8);
            result.push((triple >> 8) as u8);
            result.push(triple as u8);
        } else if chunk.len() == 3 {
            result.push((buf[0] << 2) | (buf[1] >> 4));
            result.push(((buf[1] & 0x0f) << 4) | (buf[2] >> 2));
        } else {
            // len == 2
            result.push((buf[0] << 2) | (buf[1] >> 4));
        }
    }
    Some(result)
}
