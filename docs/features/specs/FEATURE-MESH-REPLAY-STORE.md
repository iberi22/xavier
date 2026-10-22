# FEATURE-MESH-REPLAY-STORE

## Overview
This feature implements a durable, local SQLite-backed storage primitive to ensure replay requests are rejected and new ones are recorded securely across the Xavier Mesh. It addresses anti-replay constraints.

## Requirements Reference
- **REQ-012**: Replay Attack Prevention
- Parent Feature: `feat-mesh-network`
- This primitive is the persistence prerequisite for gap **G04**.

## Storage Contract and Limitations
- The store explicitly uses a caller-supplied `Path` pointing to a SQLite database. There is no automatic startup mapping or default fallback directory.
- A single monotonic `floor` row is maintained to aggressively reject stale timestamps and bound database size.
- Pruning of stale records automatically occurs during insertion of fresh, non-stale timestamps.
- **Constraints**:
  - Maximum 10,000 entries per sender. Rejects operations with `Full` if this capacity is met without a successful delete to make room.
  - Senders can be max 128 bytes (UTF-8).
  - Nonces can be max 64 bytes (UTF-8).
  - Timestamp skew must be `<= 300` seconds from the locally supplied `now`.

## Out of Scope
- This store **trusts signed timestamps ONLY after** the future caller verifies signatures, peer identity, and workspace authorization.
- The store **does not perform** any authentication, decryption, or application of the underlying payload.
- This is strictly a local storage primitive; no synchronization or network transport (`iroh_transport`) logic is enabled or integrated in this iteration. Future integration must first check receiver-controlled peer/workspace policies before issuing a claim, and decrypt/apply only upon receiving a `Fresh` claim result. Missing the store means no apply operation should proceed.