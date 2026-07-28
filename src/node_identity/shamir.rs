//! Shamir secret sharing over GF(256) — 2-of-3 split of 32-byte entropy.
//!
//! Field arithmetic matches AES GF(2^8) / Rijndael (poly 0x11b).
//! Spike reference: `docs/SWAL/spikes/sealed-pack/shamir_dek_spike.mjs`.

use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// One Shamir share: x-coordinate in 1..=n and 32 secret bytes (ys).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShamirShare {
    pub x: u8,
    pub ys: [u8; 32],
}

/// Shamir 2-of-3 (threshold k=2, n=3).
pub struct ShamirSplit;

impl ShamirSplit {
    pub fn split_2_of_3(secret: &[u8; 32]) -> Result<Vec<ShamirShare>> {
        Self::split(secret, 2, 3)
    }

    pub fn split(secret: &[u8; 32], k: u8, n: u8) -> Result<Vec<ShamirShare>> {
        if !(2..=n).contains(&k) || n > 254 {
            anyhow::bail!("invalid Shamir parameters k={k} n={n}");
        }
        let mut shares: Vec<ShamirShare> = (1..=n)
            .map(|x| ShamirShare {
                x,
                ys: [0u8; 32],
            })
            .collect();

        for (byte_idx, &secret_byte) in secret.iter().enumerate() {
            let mut coeffs = vec![secret_byte];
            for _ in 1..k {
                let mut b = [0u8; 1];
                rand::rngs::OsRng.fill_bytes(&mut b);
                // avoid zero leading coefficient for full degree (optional)
                coeffs.push(b[0]);
            }
            for share in &mut shares {
                share.ys[byte_idx] = eval_poly(&coeffs, share.x);
            }
        }
        Ok(shares)
    }

    /// Combine any ≥k distinct shares (for 2-of-3, pass ≥2 shares).
    pub fn combine(shares: &[ShamirShare]) -> Result<[u8; 32]> {
        if shares.len() < 2 {
            anyhow::bail!("need at least 2 shares to reconstruct (2-of-3)");
        }
        let mut seen = std::collections::HashSet::new();
        for s in shares {
            if s.x == 0 {
                anyhow::bail!("share x must be non-zero");
            }
            if !seen.insert(s.x) {
                anyhow::bail!("duplicate share x={}", s.x);
            }
        }
        let mut secret = [0u8; 32];
        for i in 0..32 {
            let pts: Vec<(u8, u8)> = shares.iter().map(|s| (s.x, s.ys[i])).collect();
            secret[i] = interpolate_at_zero(&pts)?;
        }
        Ok(secret)
    }
}

fn eval_poly(coeffs: &[u8], x: u8) -> u8 {
    // Horner: ((c_n * x + c_{n-1}) * x + ... ) + c_0
    let mut y = 0u8;
    for &c in coeffs.iter().rev() {
        y = gf_mul(y, x) ^ c;
    }
    y
}

/// Lagrange interpolation at x=0 over GF(256).
fn interpolate_at_zero(points: &[(u8, u8)]) -> Result<u8> {
    let mut secret = 0u8;
    for (i, &(xi, yi)) in points.iter().enumerate() {
        let mut num = 1u8;
        let mut den = 1u8;
        for (j, &(xj, _)) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            // L_i(0) = Π (0-x_j)/(x_i-x_j) = Π x_j / (x_i⊕x_j)  in char 2
            num = gf_mul(num, xj);
            den = gf_mul(den, xi ^ xj);
        }
        let li = gf_mul(num, gf_inv(den)?);
        secret ^= gf_mul(yi, li);
    }
    Ok(secret)
}

// —— GF(256) ——

fn gf_mul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 {
            p ^= a;
        }
        let hi = a & 0x80;
        a <<= 1;
        if hi != 0 {
            a ^= 0x1b;
        }
        b >>= 1;
    }
    p
}

fn gf_inv(a: u8) -> Result<u8> {
    if a == 0 {
        return Err(anyhow!("gf inv of 0"));
    }
    // Fermat: a^{254} = a^{-1} in GF(2^8)*
    let mut acc = 1u8;
    let mut base = a;
    let mut e = 254u16;
    while e > 0 {
        if e & 1 != 0 {
            acc = gf_mul(acc, base);
        }
        base = gf_mul(base, base);
        e >>= 1;
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf_mul_inv_smoke() {
        assert_eq!(gf_mul(0, 5), 0);
        assert_eq!(gf_mul(1, 5), 5);
        assert_eq!(gf_mul(0x53, 0xca), 0x01); // known AES vector: 0x53 * 0xCA = 0x01
        for a in 1..=255u8 {
            let inv = gf_inv(a).unwrap();
            assert_eq!(gf_mul(a, inv), 1, "inv({a})");
        }
    }

    #[test]
    fn shamir_2_of_3_any_pair() {
        let mut secret = [0u8; 32];
        secret[0] = 0xAB;
        secret[1] = 0xCD;
        for (i, b) in secret.iter_mut().enumerate().skip(2) {
            *b = i as u8;
        }
        let shares = ShamirSplit::split_2_of_3(&secret).unwrap();
        assert_eq!(shares.len(), 3);
        assert_eq!(ShamirSplit::combine(&shares[0..2]).unwrap(), secret);
        assert_eq!(ShamirSplit::combine(&shares[1..3]).unwrap(), secret);
        assert_eq!(
            ShamirSplit::combine(&[shares[0].clone(), shares[2].clone()]).unwrap(),
            secret
        );
        assert!(ShamirSplit::combine(&shares[0..1]).is_err());
    }

    #[test]
    fn poly_eval_constant() {
        assert_eq!(eval_poly(&[0xAB], 1), 0xAB);
        assert_eq!(eval_poly(&[0xAB], 7), 0xAB);
    }
}
