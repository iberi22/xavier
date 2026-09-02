# ADR-018: Espacio Architecture — Namespaces vs Mesh Isolation Model

> **Status:** PROPOSED (2026-09-02) · **Deciders:** BELA · **Wave:** WAVE-8.05

## Context

Xavier's `src/espacio/` module (12 files, ~2254 LoC) was committed in `342a9760` as part of WAVE-7
housekeeping, ahead of its runtime wiring. Espacios are the SWAL **nueva web espacios** primitive
(formerly "multiverso"; renamed per 02sep26 directive): isolated workspaces grouping a set of mesh
nodes with their own RAG context.

The architectural question is how espacios relate to existing SWAL mesh namespaces and identity:

- **Mesh namespaces** (`swal/{app_id}/{instance_id}`) are wire-level identity labels used for
  ACL and gossip routing in `src/mesh/`.
- **Data node consent** (`feat-data-node-consent`) controls whether a node's memory is shared
  with peers based on `pro_gate.rs`.
- **Espacios** introduce a higher-level grouping primitive that combines several nodes into a
  logical "space" with channel/invite/permission semantics.

Without a documented decision, callers may mix the three primitives inconsistently.

## Decision

We adopt the following model:

1. **Espacios sit alongside (not inside) mesh namespaces.** A espacio is identified by a
   user-meaningful slug (`swal-edu-colegio-2026`), distinct from the wire-level mesh namespace.
   Mapping is many-to-many: one espacio contains N mesh nodes; one mesh node can belong to M espacios.

2. **Identity isolation via `swal/{app_id}/{instance_id}` namespace.** A espacio's data is
   scoped to a single app namespace; cross-app espacios require explicit bridging via invites.

3. **Channel/invite/permission trust boundaries**:
   - **Channel** (`src/espacio/channel.rs`): logical message channel within a espacio. Membership is
     controlled by the espacio's permission set.
   - **Invite** (`src/espacio/invite.rs`): signed token bound to a (espacio, expiry, capabilities)
     triple. Verification uses the same Ed25519 keypair as `src/node_identity/`.
   - **Permissions** (`src/espacio/permissions.rs`): capability check on every channel action
     (`read`, `write`, `invite`, `admin`).

4. **P2P vs public visibility**:
   - **P2P** (`src/espacio/p2p.rs`): espacio is reachable only via mesh — no public ingress.
   - **Public** (`src/espacio/public.rs`): espacio has a discoverable surface, indexed by
     `src/espacio/search.rs` (RAG over public channels only).

## Consequences

**Positive:**
- Clear separation: mesh = transport identity, espacio = user-facing workspace, identity = auth.
- Capability-based permissions replace ad-hoc ACLs.
- P2P and public spaces share the same channel/invite primitives but differ in discoverability.

**Negative:**
- Two-layer mapping (espacio↔mesh) increases cognitive overhead for new contributors.
- Bridging cross-app espacios requires explicit invite flows, slowing collaboration across teams.

**Mitigations:**
- Documentation in `docs/architecture/espacio-guide.md` (companion to this ADR).
- Feature `feat-espacio-runtime-routes` (#1807) exposes the model via HTTP API.
- Tests in `feat-espacio-permissions-tests` (#1814) and `feat-espacio-channel-lifecycle` (#1815).

## Alternatives Considered

- **Single-layer (espacio == mesh namespace)**: rejected — collapses user-meaningful grouping
  into wire identity, makes cross-app collaboration impossible.
- **Trust levels in mesh itself**: rejected — duplicates the capability model in `pro_gate.rs`.

## References

- Plan: `NUEVA_WEB_ESPACIOS_XAVIER_2026-09-02.md`
- Module commit: `342a9760` (WAVE-7 housekeeping)
- Runtime wiring: #1807, #1808 (WAVE-8.01-8.02)
- Tests: #1814, #1815 (WAVE-8.03-8.04)
- Search wiring: #1813 (WAVE-8.10)
- ADR cross-ref: `docs/adr/README.md` index
