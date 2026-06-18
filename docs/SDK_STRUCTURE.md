# Xavier Sovereign Mesh SDK Structure

To support a multi-platform, sovereign Xavier network, the mesh logic will be organized into the following logical boundaries (crates/packages):

## 1. `xavier-protocol` (Crate)
- **Scope**: Core message types, Capability Token definitions, and cryptographic primitives (Ed25519, Kyber, Dilithium).
- **Dependencies**: `serde`, `oqs`, `ed25519-dalek`.
- **Visibility**: Public (published to crates.io).

## 2. `xavier-mesh-core` (Crate)
- **Scope**: Node identity management, ACL logic, Capability validation, Tokenomics (XP, Wallet), and Data Sanitization.
- **Dependencies**: `xavier-protocol`, `sqlx` (for ledger), `tracing`.
- **Visibility**: Public/Internal.

## 3. `xavier-transport-http` / `xavier-transport-iroh` (Crates)
- **Scope**: Network-specific transport implementations for Mesh messages.
- **Dependencies**: `xavier-protocol`, `reqwest` / `iroh`.
- **Visibility**: Internal.

## 4. `xavier-adapter-react` (NPM Package)
- **Scope**: React hooks and providers for interacting with a local Xavier node's mesh features from a browser/Tauri UI.
- **Dependencies**: Tauri APIs, `xavier-protocol` (WASM).
- **Visibility**: Public.

## 5. `xavier-cli` (Crate)
- **Scope**: CLI commands for managing the mesh (id, sync, peers, tokens).
- **Dependencies**: All of the above.

---

### Migration Path for `src/mesh`
Currently, `src/mesh` is a flat module within the main `xavier` library. We will gradually transition to the structure above by:
1. Isolating protocol types into `src/mesh/protocol.rs` (Done).
2. Moving security/ACL logic to a dedicated sub-module (In Progress).
3. Extracting transport-specific logic (In Progress).
