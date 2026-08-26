## 2026-08-26 - Insecure fallback for WebAuthn PRF
**Vulnerability:** Used `Math.random()` as a fallback mechanism for key generation when Web Crypto or CSPRNG wasn't available.
**Learning:** This approach produced deterministic and highly guessable `device_key` sequences in Node/headless mode.
**Prevention:** Key generation libraries should always fail hard and throw an error when a cryptographically secure random number generator is unavailable instead of trying to fall back to weaker pseudorandom mechanisms.
