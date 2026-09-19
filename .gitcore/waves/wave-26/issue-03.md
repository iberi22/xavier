# [WAVE-26.03] feat-telecom-node-session — Secure P2P telecom session state machine & handshake

> Wave 26 — Private Telecom & Autonomous Node Mesh. Labels: `wave-26`, `ola26` (sin `jules` todavía)
> Merge order: 3/13 | Risk: MED | Effort: Medium 2-4h

---

## Current State (MEDIBLE)

- File: `src/telecom/session.rs` (0 lines - file to be created as isolated island)
- Feature: `feat-private-telecom` at 0% in `.gitcore/features.json` (planned, REQ-040)
- Existing mesh: `src/mesh/private_mesh.rs` has PrivateSyncPayload with ChaCha20-Poly1305 and vector exchange.
- Need: Peer session lifecycle: connection establishment, bidirectional challenge-response, keep-alive heartbeat, and automated session key rotation.

## Desired State (DELTA)

- **Target file**: `src/telecom/session.rs`
- Implement structs: `PeerSession, SessionState, HandshakeChallenge, KeepAliveBeacon`
- Implement functions: `initiate_handshake, complete_handshake, record_heartbeat, is_active`
- Section logic:
  - High-performance, zero-allocation serialization / execution where possible.
  - Safe error handling using `thiserror` for module errors and `anyhow::Result` for fallible workflows.
  - Full adherence to asynchronous Tokio primitives (`tokio::sync::mpsc`, `tokio::sync::RwLock`).
  - Strict integration with SQLite WAL and `PRAGMA busy_timeout = 5000` to prevent database locks.
- **New tests**: Include unit tests under `#[cfg(test)] mod tests` in `src/telecom/session.rs` with >= 3 distinct test cases.
- Risk: MED — ensure zero interference with existing vector indexing or memory search pipelines.

## 🌐 Web Research Required

**MANDATORY — 4 queries. El agente DEBE investigar antes de implementar.**
1. search: "Rust ChaCha20Poly1305 ratcheting protocol 2025"
2. search: "Rust axum websocket bidirectional real-time 2025"
3. search: "P2P encrypted messaging state machine Rust 2026"
4. search: "Xavier memory node identity Ed25519 BIP39-24"

## 🔬 Agent Session Prompt

"Before implementing, please:
1. Research the topics above — search the web for current best practices (2025/2026).
2. Read and understand these existing files:
   - `src/mesh/private_mesh.rs` — understand existing ChaCha20-Poly1305 payload encryption and P2P vector sync.
   - `src/node_identity/` — understand Ed25519 node identity and BIP39 keys.
   - `src/security/clearance.rs` — understand ClearanceLevel and ClearanceEnforcer.
   - `src/storage/pragma.rs` — understand WAL and busy_timeout pragma application.
3. Check for API changes in `cargo` crates (e.g. `axum 0.8`, `tokio 1.43`, `chacha20poly1305 0.10`).
4. Ensure your implementation lives strictly in `src/telecom/session.rs` without modifying other files."

## Existing Code Patterns (MUST follow)

- `src/mesh/private_mesh.rs` → ChaCha20-Poly1305 encryption, AAD tagging, and serde serialization.
- `src/security/clearance.rs` → ClearanceLevel + ClearanceEnforcer + Role validation.
- `src/storage/pragma.rs` → `apply_pragmas` with WAL mode and `PRAGMA busy_timeout = 5000;`.
- `src/telecom/mod.rs` → Clean module re-exports.

## Acceptance Criteria (VERIFICABLES POR COMANDO)

- [ ] `cargo check -p xavier --all-targets 2>&1 | grep ^error | wc -l` == 0
- [ ] `grep -cE "PeerSession|initiate_handshake" src/telecom/session.rs` >= 1
- [ ] `cargo test --lib telecom::session -- --nocapture 2>&1 | grep ok` >= 1
- [ ] `cargo clippy --all-targets -- -D warnings 2>&1 | grep "^error" | wc -l` == 0
- [ ] `gh pr view <NUM> --json files --jq '.files | length'` >= 1 (PR contains files)

## Files to Modify

| File | Current State | Change | Risk |
|------|--------------|--------|------|
| `src/telecom/session.rs` | NEW (0 lines) | Implement `PeerSession, SessionState, HandshakeChallenge, KeepAliveBeacon` and `initiate_handshake, complete_handshake, record_heartbeat, is_active` | MED |

## DO NOT touch (Anti-Regression)

- `crates/xavier-core-logic` — core vector primitives must remain untouched.
- `.gitcore/features.json` — reconciled at wave end only by the orchestrator.
- `Cargo.lock` unless strictly required for a new dependency.
- Any other file in `src/telecom/` assigned to parallel wave tasks (maintain disjoint file islands).

## Anti-Hallucination Guard ⚠️

1. **READ before write**: Inspect existing mesh code in `src/mesh/private_mesh.rs` before coding.
2. **Match existing patterns**: Use `thiserror` for library error enums and `anyhow::Result` for high-level operations.
3. **No inventar imports**: Only use crates already in `Cargo.toml` (`tokio`, `serde`, `chacha20poly1305`, `ed25519-dalek`, `rusqlite`, `axum`).
4. **Cargo check gate**: `cargo check -p xavier --all-targets` must produce 0 errors before committing.
5. **No tocar `.gitcore/features.json`**: Reconciled only after all wave PRs are merged.

## PR Delivery Requirements (ANTI-EMPTY-PR) — OBLIGATORIO

- [ ] `git status --porcelain` shows `src/telecom/session.rs` modified/untracked BEFORE opening PR.
- [ ] `git diff --stat HEAD` lists files (NOT empty).
- [ ] The PR MUST contain >= 1 file: verify with `git ls-files` before pushing.
- [ ] IF the task cannot be finished: DO NOT open PR — leave a comment detailing blockers on the issue.
- [ ] `git show HEAD --name-only | grep -cE "src/|tests/"` >= 1 (real source files).
- [ ] `wc -l src/telecom/session.rs` >= 50 (non-trivial implementation).
- [ ] `grep -cE "fn test_|#\[test\]" src/telecom/session.rs` >= 2 (real unit tests).

## Verification

```bash
cargo check -p xavier --all-targets 2>&1 | grep ^error | wc -l  # expect 0
cargo clippy --all-targets -- -D warnings 2>&1 | grep "^error" | wc -l  # expect 0
cargo test --lib telecom::session -- --nocapture 2>&1 | tail -5  # N passed
cargo fmt --check  # 0 diff
```

## Dependencies & Merge Order

- **Depends on:** Wave 25 (committed to main)
- **Parallel with:** Wave 26 issues (disjoint file islands verified)
- **Merge order within wave:** 3/13
- **Expected effort:** Medium 2-4h

## Failure Recovery

| If this happens | Action |
|----------------|--------|
| `cargo check` fails | Fix compilation errors, do NOT commit broken code |
| `cargo test` fails on sqlite | Ensure `apply_pragmas` sets WAL mode and 5000ms busy timeout |
| Missing crate | Check if dependency already exists in workspace Cargo.toml |
| PR conflicts with parallel work | Rebase on main (`git pull --rebase origin main`), verify file island isolation |
