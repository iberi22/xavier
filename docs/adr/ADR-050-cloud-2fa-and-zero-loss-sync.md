# ADR-050 — Cloud 2FA, Multi-Tenant Persistence & Zero-Loss Context Capsule Sync

| Field | Value |
|-------|-------|
| **ID** | ADR-050 |
| **Status** | Accepted (2026-11-05) |
| **Date** | 2026-11-05 |
| **Authors** | Xavier Core Security & Infrastructure Team (SWAL) |
| **Related** | ADR-008 (decentralized login), ADR-016 (canonical data dir), ADR-037 (rate limiting), ADR-043 (master key fail-closed), ADR-048 (cloudflare passwordless auth), ADR-049 (multimodal embeddings), `docs/ARCHITECTURE/CLOUDFLARE_ZERO_TRUST_PARTNERSHIP.md`, `docs/SRS/REQUIREMENTS.md` |
| **Durable path** | `docs/adr/ADR-050-cloud-2fa-and-zero-loss-sync.md` |

---

## 1. Context & Problem Statement

As part of Xavier's Ola 4 development, cross-device AI agent operations (Windows development nodes, remote coding agents, Linux hosts, and web operators) require distributed access to cognitive memory without compromising the core principles of SWAL: local-first execution, zero-knowledge cloud storage, and defense-in-depth credential protection.

Prior to Ola 4:
1. **Authentication Friction vs Security:** Access to remote or cloud-hosted instances relied on either static pre-shared tokens or single-factor passwords, creating vulnerability to token replay, credential stuffing, and session theft.
2. **Key Material Exposure:** Browser web clients and developer CLI environments risked exposing master tokens or decryption keys in terminal command history, environment dumps, or unencrypted local configuration files.
3. **Multi-Tenant State Leakage & Backup:** Storage backups sent to remote cloud backends risked exposing plain text vector memories and embeddings to cloud operators and infrastructure providers.
4. **Context Degradation & Handoff Loss:** Long-running agent reasoning sessions (e.g. >250k–300k tokens or >750 steps) suffered from token budget exhaustion and prompt drift, lacking an authenticated, tamper-proof, zero-loss snapshot and synchronization mechanism across devices.

A formal architectural decision is required to unify:
- Multi-factor authentication via Cloud 2FA (TOTP) and passwordless browser magic links.
- Key isolation using `HardwareVault` token encryption backed by OS-level secure storage with fallback encrypted envelopes.
- Multi-tenant persistence over Cloudflare R2 with zero-knowledge guarantees via Argon2id + AES-256-GCM and Shamir 2-of-3 secret sharing.
- Zero-loss context capsule synchronization across private mesh and cloud tiers.

---

## 2. Competing Architectural Options

| Option | Description | Trade-offs & Complexity |
|--------|-------------|-------------------------|
| **Option A: Centralized IdP & Server-Side Cloud Storage** | Centralized auth service (e.g. Auth0/Cognito) issuing bearer JWTs with memory snapshots stored in standard S3 buckets encrypted with cloud provider keys (SSE-S3/KMS). | **Low local engineering effort, but unacceptable architectural compromise.** Violates zero-knowledge and local-first requirements. Cloud provider or compromised operator has full read access to private agent memory. Incurs significant egress and per-seat SaaS costs. |
| **Option B: Pure Local P2P Sync (No Cloud Relay)** | Pure WebRTC / libp2p gossip mesh synchronization between active nodes with manual secret transfer and file-based TOTP. | **Zero cloud dependency, but poor UX and offline availability.** Mobile devices and remote coding sandboxes cannot sync when the host workstation is sleeping or behind symmetric NATs without relay infrastructure. |
| **Option C: Hybrid Zero-Knowledge Cloud Relay + HardwareVault + Shamir 2-of-3 (Selected)** | Cloudflare edge authentication (Access + Turnstile + Magic Link + RFC 6238 TOTP) coupled with client-side zero-knowledge encryption before sync to Cloudflare R2 (`xaviercloud-packs`). Session tokens and master credentials encrypted in `HardwareVault`. Node recovery secured via Shamir 2-of-3 threshold split. Agent context snapshots packaged into authenticated `.swal_capsule` archives. | **Optimal balance.** Near-zero egress cost via Cloudflare R2, zero-knowledge privacy guarantee (cloud operator sees only ciphertext), seamless multi-tenant isolation, and resilient recovery without single points of compromise. |

---

## 3. Decision

We adopt **Option C: Hybrid Zero-Knowledge Cloud Relay, HardwareVault Token Isolation, and Zero-Loss Context Capsule Synchronization**.

Specifically:

### 3.1 Cloud 2FA & Authenticated Web Magic Links
1. Multi-factor authentication is enforced using standard RFC 6238 Time-based One-Time Passwords (TOTP) with HMAC-SHA1/SHA256, 30-second time steps, and a ±1 window tolerance.
2. User 2FA enrollment is cryptographically bound to the authenticated identity (`claims.sub` in JWT); enrollment operations for one user cannot mutate or overwrite another user's TOTP secret or backup codes.
3. Interactive login via CLI (`xavier login`) supports automated 1-click browser magic-link authorization exchanging single-use cryptographically random tokens (CSPRNG, 32 bytes) over Cloudflare edge with Turnstile verification.

### 3.2 HardwareVault Token Encryption
1. Session tokens (`XAVIER_CLOUD_TOKEN`, `PGHEART_TOKEN`), API keys, and database encryption keys must never be stored in plain text configuration files, terminal history, or git repositories.
2. Tokens are stored through `HardwareVault` (`src/secrets/vault.rs`), utilizing platform-native keyrings (Linux Secret Service / Keyutils, macOS Keychain, Windows Credential Manager).
3. In headless or containerized environments without an OS keyring, `HardwareVault` falls back to machine-bound AES-256-GCM encrypted fallback storage (`0600` permissions on secret files, `0700` on containing directories) with keys derived from machine-specific hardware entropy (`HKDF-SHA256`).

### 3.3 Multi-Tenant Persistence & Zero-Knowledge Guarantees
1. Memory archives and snapshots synchronized to Cloudflare R2 (`xaviercloud-packs`) are sealed on the client before network transmission using the `XCB1` container format.
2. Encryption utilizes **Argon2id** (memory cost 19,456 KiB, 2 iterations, 1 lane) for key derivation from a user passphrase, followed by **AES-256-GCM** authenticated payload encryption with unique 96-bit nonces.
3. Multi-tenant quota enforcement and rate limiting are handled at Cloudflare Edge Workers via SHA-256 hashed tenant IDs (`XAVIER_CLOUD_KV`), preventing denial-of-wallet without Cloudflare ever having access to the unencrypted plaintext or memory keys.

### 3.4 Shamir 2-of-3 Entropy Split & Recovery
1. Node identity master entropy (32 bytes) is split using Shamir's Secret Sharing over $\text{GF}(2^8)$ into 3 independent threshold shares $(k=2, n=3)$:
   - **Share 1 (Local):** Stored in local encrypted HardwareVault.
   - **Share 2 (Cloud Backup):** Client-side encrypted with user passphrase and persisted to cloud/R2.
   - **Share 3 (Paper / Cold Storage):** Exported as offline recovery check-codes.
2. Any 2 of the 3 shares suffice to deterministically reconstruct the master entropy and derive all subkeys; a single compromised share yields mathematical zero information regarding the underlying secret.

### 3.5 Zero-Loss Context Capsule Synchronization (`.swal_capsule`)
1. When reasoning sessions reach high context pressure, memory state, active agent transcripts, belief graph snapshots, and code graph positions are compiled into an immutable context capsule.
2. The capsule is serialized and encrypted via `AirgapCapsule` (`src/crypto/airgap_capsule.rs`), enforcing a 1 GiB safety ceiling, atomic SHA-256 integrity digest validation, and safe extraction (`O_NOFOLLOW` symlink traversal prevention).
3. New agent sessions resume seamlessly by referencing the immutable capsule identifier (`xavier search <capsule_id>`), recovering deep cognitive context with zero degradation.

---

## 4. Consequences

### Positive
- **Guaranteed Privacy:** Zero-knowledge guarantees ensure that even in the event of a Cloudflare bucket compromise or subpoena, stored memory packs remain unreadable ciphertext.
- **Robust Key Security:** Operating system-level hardware vault isolation prevents accidental token leakage via dotfile commits or process environment inspection.
- **Lossless Agent Continuity:** Context capsules prevent reasoning degradation and token exhaustion across multi-hour agent workflows.
- **Predictable Zero-Egress Costs:** Leveraging Cloudflare R2 eliminates egress fees, providing high gross margin (>98%) on multi-tenant synchronization.

### Negative & Mitigations
- **Complexity of Recovery:** Users must safeguard at least 2 of 3 Shamir shares or their passphrase; permanent data loss occurs if 2 shares are lost. *Mitigation:* Explicit CLI warnings and guided paper backup printouts during node initialization.
- **Headless Keyring Fallback:** Container environments lacking D-Bus/keyring require local machine-bound fallback. *Mitigation:* Automated permission enforcement (`0600`/`0700`) and fail-closed master key initialization.

---

## 5. Verification & Observability Metrics

The success and enforcement of this decision are verified by:

1. **Authentication Scoping & Isolation:**
   - Unit & integration tests in `tests/test_auth_2fa_scoped.rs` and `src/auth2/` validating that 2FA enrollment is strictly isolated to the caller's JWT subject (`claims.sub`).
   - Browser magic link redirection and token exchange tests in `tests/` verifying token extraction from URL fragments and secure cookie storage.
2. **HardwareVault Isolation & Permissions:**
   - Tests asserting that no plain text secrets exist in configuration files after `save_session_credentials`.
   - File permission assertions confirming `0600` on fallback secret files and `0700` on directories.
3. **Capsule Integrity & Size Limits:**
   - Test suites in `src/cli/commands/airgap.rs` asserting rejection of oversized capsules (>1 GiB), corrupted digests, and unsafe symlink traversal.
4. **Shamir Split & Combine Roundtrips:**
   - Cryptographic tests in `src/node_identity/shamir.rs` validating that any 2 shares reconstruct the exact 32-byte secret, while any single share fails reconstruction.
