//! Publisher identity for marketplace mutations.
//!
//! A publisher's identity is an ed25519 public key, written as lowercase hex.
//! Create, update and revoke must carry a signature made with that key over
//! the request: method, path, timestamp and the SHA-256 of the body. The
//! identity is therefore proven on every request, not asserted by a body field
//! and not implied by a shared bearer token.
//!
//! [`Actor`] has no public constructor except [`Actor::verify`], so a store
//! method that takes `&Actor` cannot be handed a self-asserted identity.

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::error::MarketError;

/// How far a request timestamp may sit from the verifier's clock, either way.
pub const MAX_CLOCK_SKEW_MS: i64 = 5 * 60 * 1000;

/// Domain separator, so a signature made for another protocol cannot be replayed here.
const DOMAIN_TAG: &str = "xavier-market/v2 request";

/// A publisher identity proven by a valid signature on this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    id: String,
}

impl Actor {
    /// The publisher id: the lowercase hex of the ed25519 public key.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Verifies a signed request and returns the identity that made it.
    pub fn verify(request: &SignedRequest<'_>, now_ms: i64) -> Result<Self, MarketError> {
        let key_bytes: [u8; 32] = decode_fixed("public key", request.public_key_hex)?;
        let signature_bytes: [u8; 64] = decode_fixed("signature", request.signature_hex)?;
        if (now_ms - request.timestamp_ms).abs() > MAX_CLOCK_SKEW_MS {
            return Err(unauthenticated(
                "request timestamp is outside the allowed window",
            ));
        }
        let key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|_| unauthenticated("public key is not a valid ed25519 key"))?;
        let message = signing_message(
            request.method,
            request.path,
            request.timestamp_ms,
            request.body,
        );
        key.verify_strict(&message, &Signature::from_bytes(&signature_bytes))
            .map_err(|_| unauthenticated("signature does not match the request"))?;
        Ok(Self {
            id: hex::encode(key_bytes),
        })
    }

    /// A publisher id without a signature. Tests only; production code gets an
    /// [`Actor`] from [`Actor::verify`].
    #[cfg(test)]
    pub(crate) fn for_test(id: &str) -> Self {
        Self { id: id.to_string() }
    }
}

/// The request fields a signature covers, plus the signature and key the client sent.
#[derive(Debug, Clone, Copy)]
pub struct SignedRequest<'a> {
    pub public_key_hex: &'a str,
    pub signature_hex: &'a str,
    pub timestamp_ms: i64,
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
}

/// The canonical bytes a signature covers.
pub fn signing_message(method: &str, path: &str, timestamp_ms: i64, body: &[u8]) -> Vec<u8> {
    let body_digest = hex::encode(Sha256::digest(body));
    format!(
        "{DOMAIN_TAG}\n{}\n{path}\n{timestamp_ms}\n{body_digest}",
        method.to_ascii_uppercase()
    )
    .into_bytes()
}

/// Header values a client attaches to a signed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedHeaders {
    pub public_key_hex: String,
    pub timestamp_ms: i64,
    pub signature_hex: String,
}

/// Client-side signer for an ed25519 publisher key.
pub struct Signer {
    key: SigningKey,
}

impl Signer {
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(self.key.verifying_key().to_bytes())
    }

    /// Signs one request. The timestamp is the caller's clock in milliseconds.
    pub fn sign(&self, method: &str, path: &str, timestamp_ms: i64, body: &[u8]) -> SignedHeaders {
        let message = signing_message(method, path, timestamp_ms, body);
        SignedHeaders {
            public_key_hex: self.public_key_hex(),
            timestamp_ms,
            signature_hex: hex::encode(self.key.sign(&message).to_bytes()),
        }
    }
}

fn decode_fixed<const N: usize>(what: &str, value: &str) -> Result<[u8; N], MarketError> {
    let bytes =
        hex::decode(value).map_err(|_| unauthenticated(format!("{what} is not valid hex")))?;
    bytes
        .try_into()
        .map_err(|_| unauthenticated(format!("{what} has the wrong length")))
}

fn unauthenticated(message: impl Into<String>) -> MarketError {
    MarketError::Unauthenticated(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000_000;
    const PATH: &str = "/v2/marketplace/listings";

    fn signer(seed: u8) -> Signer {
        Signer::from_seed([seed; 32])
    }

    fn verify_with(
        headers: &SignedHeaders,
        method: &str,
        path: &str,
        body: &[u8],
        now_ms: i64,
    ) -> Result<Actor, MarketError> {
        Actor::verify(
            &SignedRequest {
                public_key_hex: &headers.public_key_hex,
                signature_hex: &headers.signature_hex,
                timestamp_ms: headers.timestamp_ms,
                method,
                path,
                body,
            },
            now_ms,
        )
    }

    #[test]
    fn a_signed_request_verifies_to_the_signing_key() {
        let key = signer(1);
        let headers = key.sign("POST", PATH, NOW, b"{}");
        let actor = verify_with(&headers, "POST", PATH, b"{}", NOW).unwrap();
        assert_eq!(actor.id(), key.public_key_hex());
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        let owner = signer(1);
        let other = signer(2);
        let mut headers = other.sign("PATCH", PATH, NOW, b"{}");
        headers.public_key_hex = owner.public_key_hex();
        assert!(matches!(
            verify_with(&headers, "PATCH", PATH, b"{}", NOW),
            Err(MarketError::Unauthenticated(_))
        ));
    }

    #[test]
    fn the_signature_covers_method_path_body_and_time() {
        let headers = signer(1).sign("PATCH", PATH, NOW, b"{\"price_credits\":5}");
        assert!(verify_with(&headers, "PATCH", PATH, b"{\"price_credits\":5}", NOW).is_ok());
        assert!(verify_with(&headers, "DELETE", PATH, b"{\"price_credits\":5}", NOW).is_err());
        assert!(verify_with(
            &headers,
            "PATCH",
            "/v2/marketplace/listings/other",
            b"{\"price_credits\":5}",
            NOW
        )
        .is_err());
        assert!(verify_with(&headers, "PATCH", PATH, b"{\"price_credits\":0}", NOW).is_err());
        let retimed = SignedHeaders {
            timestamp_ms: NOW + 1,
            ..headers.clone()
        };
        assert!(verify_with(&retimed, "PATCH", PATH, b"{\"price_credits\":5}", NOW).is_err());
    }

    #[test]
    fn timestamps_outside_the_skew_window_are_refused() {
        let headers = signer(1).sign("POST", PATH, NOW, b"");
        assert!(verify_with(&headers, "POST", PATH, b"", NOW + MAX_CLOCK_SKEW_MS).is_ok());
        assert!(verify_with(&headers, "POST", PATH, b"", NOW - MAX_CLOCK_SKEW_MS).is_ok());
        assert!(verify_with(&headers, "POST", PATH, b"", NOW + MAX_CLOCK_SKEW_MS + 1).is_err());
        assert!(verify_with(&headers, "POST", PATH, b"", NOW - MAX_CLOCK_SKEW_MS - 1).is_err());
    }

    #[test]
    fn malformed_key_or_signature_is_refused() {
        let good = signer(1).sign("POST", PATH, NOW, b"");
        let bad_hex = SignedHeaders {
            public_key_hex: "not-hex".to_string(),
            ..good.clone()
        };
        let short_key = SignedHeaders {
            public_key_hex: "abcd".to_string(),
            ..good.clone()
        };
        let short_sig = SignedHeaders {
            signature_hex: "00".repeat(10),
            ..good.clone()
        };
        for headers in [bad_hex, short_key, short_sig] {
            assert!(matches!(
                verify_with(&headers, "POST", PATH, b"", NOW),
                Err(MarketError::Unauthenticated(_))
            ));
        }
    }

    #[test]
    fn uppercase_key_hex_yields_the_canonical_lowercase_id() {
        let key = signer(3);
        let mut headers = key.sign("POST", PATH, NOW, b"");
        headers.public_key_hex = headers.public_key_hex.to_uppercase();
        let actor = verify_with(&headers, "POST", PATH, b"", NOW).unwrap();
        assert_eq!(actor.id(), key.public_key_hex());
    }
}
