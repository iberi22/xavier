//! MinHash implementation for near-duplicate detection
//!
//! Provides MinHash signature generation and Jaccard similarity computation
//! using 128 permutations for fast document comparison.

use xxhash_rust::xxh3::xxh3_64_with_seed;
use std::collections::HashSet;

pub const MINHASH_PERMUTATIONS: usize = 128;

/// Computes the MinHash signature for a given text.
/// Uses 128 permutations (simulated via seeds) and XXH3 hashing.
pub fn compute_minhash(text: &str) -> Vec<u64> {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return vec![u64::MAX; MINHASH_PERMUTATIONS];
    }

    let mut signature = vec![u64::MAX; MINHASH_PERMUTATIONS];

    for token in tokens {
        let bytes = token.as_bytes();
        for (i, sig_val) in signature.iter_mut().enumerate().take(MINHASH_PERMUTATIONS) {
            let hash = xxh3_64_with_seed(bytes, i as u64);
            if hash < *sig_val {
                *sig_val = hash;
            }
        }
    }

    signature
}

/// Computes the Jaccard similarity between two MinHash signatures.
pub fn jaccard_similarity(a: &[u64], b: &[u64]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut matches = 0;
    for i in 0..a.len() {
        if a[i] == b[i] && a[i] != u64::MAX {
            matches += 1;
        }
    }

    matches as f32 / a.len() as f32
}

fn tokenize(value: &str) -> HashSet<String> {
    value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() >= 3)
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minhash_identity() {
        let text = "Xavier is a cognitive memory system for AI agents.";
        let sig1 = compute_minhash(text);
        let sig2 = compute_minhash(text);
        assert_eq!(sig1, sig2);
        assert_eq!(jaccard_similarity(&sig1, &sig2), 1.0);
    }

    #[test]
    fn test_minhash_similarity() {
        let text1 = "Xavier is a cognitive memory system for AI agents.";
        let text2 = "Xavier is a cognitive memory system for artificial intelligence agents.";
        let sig1 = compute_minhash(text1);
        let sig2 = compute_minhash(text2);
        let sim = jaccard_similarity(&sig1, &sig2);
        assert!(sim > 0.5);
        assert!(sim < 1.0);
    }

    #[test]
    fn test_minhash_difference() {
        let text1 = "Xavier is a cognitive memory system for AI agents.";
        let text2 = "The quick brown fox jumps over the lazy dog.";
        let sig1 = compute_minhash(text1);
        let sig2 = compute_minhash(text2);
        let sim = jaccard_similarity(&sig1, &sig2);
        assert!(sim < 0.1);
    }

    #[test]
    fn test_empty_text() {
        let sig = compute_minhash("");
        assert_eq!(sig.len(), MINHASH_PERMUTATIONS);
        assert!(sig.iter().all(|&h| h == u64::MAX));
    }
}
