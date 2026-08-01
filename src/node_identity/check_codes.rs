//! Recovery check-codes: HMAC-SHA256(seed, "swal-recovery-v1") → 6×3-digit triplets.
//! Ordered ASC/DESC challenge for recovery session (§4.3).

use crate::crypto::hmac::hmac_sha256;
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"swal-recovery-v1";

/// Six triplets (000–999) derived from the BIP39 seed bytes.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckCodes {
    pub triplets: [u16; 6],
}

impl CheckCodes {
    pub fn from_seed_bytes(seed_bytes: &[u8; 64]) -> Self {
        let mac = hmac_sha256(seed_bytes, DOMAIN);
        // Interpret first 18 nibbles-ish as 6×3 decimal digits via base-1000 chunks
        let mut triplets = [0u16; 6];
        for i in 0..6 {
            let offset = i * 3;
            let v = u32::from(mac[offset])
                | (u32::from(mac[offset + 1]) << 8)
                | (u32::from(mac[offset + 2]) << 16);
            triplets[i] = (v % 1000) as u16;
        }
        Self { triplets }
    }

    /// Human-readable `NNN-NNN-…` form.
    pub fn display_joined(&self) -> String {
        self.triplets
            .iter()
            .map(|t| format!("{t:03}"))
            .collect::<Vec<_>>()
            .join("-")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderMode {
    Asc,
    Desc,
}

/// Session challenge: shuffled display + required ASC or DESC order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderedChallenge {
    pub mode: OrderMode,
    /// Triplets shown to the user in random order (same multiset as CheckCodes).
    pub displayed: [u16; 6],
}

impl OrderedChallenge {
    pub fn new(mode: OrderMode, codes: &CheckCodes) -> Self {
        let mut displayed = codes.triplets;
        displayed.shuffle(&mut rand::thread_rng());
        Self { mode, displayed }
    }

    pub fn random(codes: &CheckCodes) -> Self {
        let mode = if rand::thread_rng().gen_bool(0.5) {
            OrderMode::Asc
        } else {
            OrderMode::Desc
        };
        Self::new(mode, codes)
    }

    pub fn expected_response(&self, codes: &CheckCodes) -> [u16; 6] {
        let mut sorted = codes.triplets;
        sorted.sort_unstable();
        match self.mode {
            OrderMode::Asc => sorted,
            OrderMode::Desc => {
                sorted.reverse();
                sorted
            }
        }
    }

    pub fn verify(&self, response: &[u16; 6], codes: &CheckCodes) -> bool {
        // Ensure displayed is a permutation of codes (anti-tamper)
        let mut a = self.displayed;
        let mut b = codes.triplets;
        a.sort_unstable();
        b.sort_unstable();
        if a != b {
            return false;
        }
        response == &self.expected_response(codes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_deterministic() {
        let seed = [7u8; 64];
        let a = CheckCodes::from_seed_bytes(&seed);
        let b = CheckCodes::from_seed_bytes(&seed);
        assert_eq!(a, b);
        assert_eq!(a.triplets.len(), 6);
        for t in a.triplets {
            assert!(t < 1000);
        }
    }

    #[test]
    fn challenge_asc_desc() {
        let codes = CheckCodes {
            triplets: [100, 50, 200, 10, 900, 300],
        };
        let asc = OrderedChallenge::new(OrderMode::Asc, &codes);
        assert_eq!(asc.expected_response(&codes), [10, 50, 100, 200, 300, 900]);
        let desc = OrderedChallenge::new(OrderMode::Desc, &codes);
        assert_eq!(desc.expected_response(&codes), [900, 300, 200, 100, 50, 10]);
        assert!(asc.verify(&asc.expected_response(&codes), &codes));
        assert!(!asc.verify(&desc.expected_response(&codes), &codes));
    }
}
