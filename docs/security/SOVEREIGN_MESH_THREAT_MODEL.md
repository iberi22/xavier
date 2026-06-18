# Xavier Sovereign Mesh Threat Model

## 1. Node Access & Identity
- **Threat**: A malicious node impersonates a trusted mirror to gain access to private memory.
- **Mitigation**: Nodes use Ed25519-based NodeIDs. All handshakes are signed and optionally use a pre-shared pairing secret. Internal access requires a valid, signed Capability Token.
- **Threat**: Compromise of a mirror node exposes replicated private data.
- **Mitigation**: Data is encrypted at rest on all nodes. Access to mirror nodes should be restricted to the same security perimeter as the main node.

## 2. Wallet & Capability Grants
- **Threat**: Capability token theft or leakage.
- **Mitigation**: Tokens are scoped to specific NodeIDs and have expiration timestamps. Tokens are never transmitted in the clear (always over TLS/encrypted transport).
- **Threat**: Unauthorized wallet creation or key derivation.
- **Mitigation**: Private keys are stored in the system keyring/TPM where available. Multi-factor authorization (MFA) for critical wallet operations (e.g., token issuance).

## 3. Data Sharing & Leakage
- **Threat**: Private memory is accidentally shared with the external network.
- **Mitigation**: Strict boundary at the `MeshTransport` layer. `DataSanitizer` middleware automatically redacts sensitive fields in any message destined for an "external" peer.
- **Threat**: De-anonymization of telemetry.
- **Mitigation**: Use of SHA-256 hashing for NodeIDs in external telemetry. Differential privacy techniques for aggregate data collection.

## 4. Reward Abuse & Governance Gaming
- **Threat**: Sybil attack on governance (one actor creating many nodes to sway votes).
- **Mitigation**: Voting weight is tied to XP balance + Reputation + Activity. New nodes have zero weight.
- **Threat**: Forging of reward-producing events.
- **Mitigation**: Append-only deterministic ledger. All rewards must be signed by the issuing node. External network can audit the ledger for inconsistencies.
- **Threat**: "Wash trading" of data (nodes buying/selling their own data to earn XP).
- **Mitigation**: Rarity-based pricing and EigenTrust reputation scores that penalize suspicious circular patterns.

## 5. Network Integrity
- **Threat**: Eclipse attack on the mesh.
- **Mitigation**: Peer discovery uses signed manifests and trusted seed nodes.
- **Threat**: Man-in-the-middle (MITM) on sync operations.
- **Mitigation**: All sync traffic is encrypted with the recipient's public key (Kyber-1024) and authenticated via signatures (Dilithium-5).
