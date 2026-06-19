//! HMAC-SHA256 Implementation (RFC 2104)

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const BLOCK_SIZE: usize = 64;

pub struct HmacSha256 {
    inner: Sha256,
    o_key_pad: [u8; BLOCK_SIZE],
}

impl HmacSha256 {
    pub fn new(key: &[u8]) -> Self {
        let mut fixed_key = [0u8; BLOCK_SIZE];
        if key.len() > BLOCK_SIZE {
            let hash = Sha256::digest(key);
            fixed_key[..hash.len()].copy_from_slice(&hash);
        } else {
            fixed_key[..key.len()].copy_from_slice(key);
        }

        let mut i_key_pad = [0u8; BLOCK_SIZE];
        let mut o_key_pad = [0u8; BLOCK_SIZE];

        for i in 0..BLOCK_SIZE {
            i_key_pad[i] = fixed_key[i] ^ 0x36;
            o_key_pad[i] = fixed_key[i] ^ 0x5c;
        }

        let mut inner = Sha256::new();
        inner.update(&i_key_pad);

        Self {
            inner,
            o_key_pad,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finalize(self) -> [u8; 32] {
        let inner_hash = self.inner.finalize();
        let mut outer = Sha256::new();
        outer.update(&self.o_key_pad);
        outer.update(&inner_hash);
        outer.finalize().into()
    }

    pub fn verify(key: &[u8], data: &[u8], signature: &[u8]) -> bool {
        let actual = hmac_sha256(key, data);
        actual.ct_eq(signature).into()
    }
}

pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hmac = HmacSha256::new(key);
    hmac.update(data);
    hmac.finalize()
}
