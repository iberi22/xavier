# ADR-019 — Plugin-First Boundary & Core Architecture

| Field | Value |
|-------|-------|
| **ID** | ADR-019 |
| **Title** | Plugin-First Boundary & Core Architecture |
| **Status** | Accepted |
| **Date** | 2026-09-16 |
| **Authors** | SWAL / Xavier Architecture |
| **Related** | ADR-007 (Code Graph Native-Only), ADR-017 (First-Class Mesh Networks), SUBAGENT-OUT/wave23-mesh-design.md |

## Status

Accepted. Non-negotiable architectural mandate by the Owner, verified 2026-09-16.

This ADR is canonical for the `main` branch of the repository.
It defines the hard boundary between the Xavier core and all out-of-core
plugins/sidecars. No code in `src/`, `Cargo.toml`, or `.gitcore/features.json`
is modified by this ADR; it is a design record only.

## Context

The root workspace has grown to approximately 258k LOC with 1426 dependent
packages. A full build takes approximately 23 minutes. This scale makes every
additional in-core dependency — native or otherwise — a direct tax on all
contributors, all CI pipelines, and all downstream applications that embed
Xavier as a library.

Two component families drove this growth without belonging to the persistence
and embeddings engine:

1. The C fork of codegraph: an external AST/indexing codebase with its own
   toolchain (C compiler, headers, codegen), ABI surface, and release cadence.
   It is a foreign fork, not Xavier code, and coupling it into the core forces
   every core build to pay for a parser toolchain it does not need.

2. The SWAL mesh / P2P / sync transports: networking stacks (HTTP sync,
   Iroh NAT-traversal, legacy libp2p shims, fallback transports, pairing,
   telemetry publish paths) with sockets, timers, background tasks, and
   network-failure modes. These are transport concerns, not storage concerns.
   The Wave-23 offline audit (`SUBAGENT-OUT/wave23-mesh-design.md`) confirms
   the mesh surface is wide (48 files under `src/mesh/`, wallet, service
   registry, telemetry, pairing) with Phase 1 HTTP operational and later phases
   (Iroh/CRDT/Tor) still pending — exactly the kind of evolving surface that
   must version independently of the storage core.

Meanwhile, the value proposition of the Xavier core is narrow and stable:
a local-first, embeddable, distributed vector database (SQLite + sqlite-vec +
FTS5), an embeddings engine, and a storage protocol that any SWAL application
can compile in and replicate between instances without friction.

## Decision

The Xavier core is restricted exclusively to the local and distributed vector
database, the embeddings engine, and the storage protocol. Everything else
leaves the core as an external plugin/sidecar behind a versioned contract
(UDS/IPC/WASM).

Concretely, the architecture is:

- **Core (in):** SQLite + sqlite-vec + FTS5 vector store, embeddings engine,
  distributed-database logic (healthy WAL, idempotent migrations, protocol
  version handshake, CODEX-blast index coordination), namespace isolation
  (`swal/{app_id}/{instance_id}`), node identity (Ed25519 BIP39-24), and the
  storage-level sync record protocol.
- **(C1) CodeGraph AST Parser (out):** external sidecar over UDS/IPC. The C
  fork lives entirely outside the core. Nothing of the C fork lives in core.
- **(C2) SWAL Mesh P2P Network (out):** external sidecar with a versioned
  contract. All transports (HTTP, Iroh, fallback, future CRDT/Tor) live behind
  the sidecar boundary.
- **(C3) Panel UI web presentation (out):** external presentation layer. No web
  presentation code constrains core builds or core releases.

Owner mandate restated as invariants (must hold on every future PR):

1. The C codegraph is a FOREIGN FORK: it is managed as a plugin maintained
   OUTSIDE the Xavier core (like everything on the SWAL network). Nothing of
   the C fork lives in the core.
2. The SWAL network (mesh/P2P/sync) leaves the core as a sidecar/plugin with a
   versioned contract (UDS/IPC/WASM).
3. The Xavier core DOES retain distributed-database logic, ready to install and
   replicate without friction between the different apps we compile
   (embeddings, sqlite-vec, CODEX-blast, healthy WAL, idempotent migrations,
   protocol version handshake).

## Structural Boundaries (Core vs Plugin)

### Core vs Plugin — membership rule

A module belongs in core if and only if it satisfies ALL of the following:

1. It runs without sockets, without subprocesses, and without a C toolchain.
2. It is required to open, read, write, migrate, or replicate a local Xavier
   database on a fresh machine with `cargo build` only.
3. Its failure modes are storage failure modes (corruption, WAL growth,
   migration conflict, version skew) — not network or parser failure modes.

Anything failing any of the three tests is a plugin.

### Core (allowed)

- `storage`: SQLite connection lifecycle, WAL checkpointing, vacuum policy,
  page-size and journal-mode discipline, corruption detection.
- `vector`: sqlite-vec index build/query, embedding dimensionality discipline,
  CODEX-blast coordination records, FTS5 hybrid ranking inputs.
- `embeddings`: local embedding provider abstraction, dimension negotiation,
  batching and caching policy (no network transport inside).
- `migrations`: idempotent, ordered, checksummed schema migrations with
  forward-only semantics and version-handshake participation.
- `replication records`: opaque sync deltas, vector clocks / sequence cursors,
  wallet-scoped isolation predicates evaluated locally (no socket IO).
- `identity/namespace`: Ed25519 node identity verification primitives and
  `swal/{app_id}/{instance_id}` namespace parsing as pure functions.
- `protocol versioning`: `PROTOCOL_VERSION` negotiation types shared with
  plugins as versioned schema, not as behavior.

### Plugin (forbidden in core)

| Plugin | Lives as | Forbidden in core |
|--------|----------|-------------------|
| C1 CodeGraph AST Parser | Sidecar process, UDS/IPC | Any C source, C headers, C build script, FFI binding, tree-sitter grammar compilation, fork-sync tooling |
| C2 SWAL Mesh P2P | Sidecar process, versioned contract | libp2p/iroh transports, dial/listen loops, NAT traversal, pairing HTTP endpoints, heartbeat tasks, telemetry publish sockets, CRDT/Tor stacks |
| C3 Panel UI | Separate web artifact | Bundlers, frontend frameworks, static assets, browser-compat shims constraining core MSRV or features |

### Dependency direction

- Core depends on nothing plugin-side. Core never imports a plugin crate,
  never `cfg`-gates plugin behavior, never shells out to a sidecar.
- Plugins depend on the core contract (schemas + protocol version), never on
  core internals. A plugin that needs a core struct field it cannot get through
  the contract files a contract-evolution proposal; it does not reach into core.
- Build isolation: `cargo build` of the core feature set must never require a
  C compiler, a Node toolchain, or network access beyond crates.io.

## Protocol Contracts

All core↔plugin interaction crosses a versioned boundary. No shared mutable
memory, no direct function calls across the boundary in-process.

### Transport options (in preference order)

1. **UDS (Unix Domain Socket)** — default for local sidecars (C1 parser, local
   mesh daemon). Length-prefixed JSON frames; one request per frame.
2. **IPC (stdin/stdout JSONL worker)** — fallback for sandboxed embeddings of
   the parser where UDS is unavailable.
3. **WASM** — future-proofing for browser/panel embedding of read-only query
   paths; no WASI filesystem escape outside the granted database handle.

### Contract envelopes

Every frame carries:

```json
{
  "protocol_version": "xavier-core/1",
  "message_id": "uuid-v4",
  "kind": "codegraph.parse.request | mesh.sync.offer | ...",
  "payload": {}
}
```

Rules:

- `protocol_version` uses `major` compatibility: a sidecar with a higher major
  than the core MUST be rejected with a typed `VersionSkew` error; a higher
  minor is accepted with unknown-field tolerance (serde `deny_unknown_fields`
  is forbidden on contract types).
- Every request has a bounded response: either `ok` with payload or a typed
  error (`VersionSkew`, `NamespaceDenied`, `CorruptDelta`, `ParserUnavailable`,
  `TransportDegraded`). No panics cross the boundary; panics are sidecar-local
  and surface as `TransportDegraded` with the `message_id` preserved.
- Timeouts are set by the caller (core never blocks indefinitely on a plugin):
  default 5s for parser calls, 10s for mesh offer/accept round-trips,
  configurable per namespace.

### C1 contract (CodeGraph parser sidecar)

- Request: `{ path, language_hint, content_hash, namespace }`. The sidecar
  returns symbols/edges as data; it never writes to the database directly.
- Core validates, embeds, and persists results through the normal storage path,
  so a malicious or stale parser cannot corrupt WAL or FTS5 state.
- Parser availability is advisory: core full-text and vector search MUST work
  with the parser absent (degraded symbol graph, intact recall base).

### C2 contract (Mesh sidecar)

- Sync unit is an opaque, signed delta batch: `{ from_seq, to_seq, namespace,
  wallet_id, payload_hash, signature }`. The sidecar transports; the core
  validates (signature, namespace, wallet isolation, sequence continuity) and
  applies idempotently.
- Cross-wallet application is rejected at the core predicate layer even if the
  sidecar delivers it — transport is untrusted by construction.
- Pairing/join, NAT traversal, and telemetry export are sidecar-internal. The
  only mesh surface the core exposes is `offer_deltas` / `accept_deltas` over
  the contract, plus read-only peer-count health for diagnostics.

### Version handshake

On sidecar startup: `hello { protocol_version, sidecar_id, capabilities[] }`
→ core answers `welcome { negotiated_version, core_capabilities[] }` or
`reject { reason: VersionSkew, min_supported, max_supported }`. Negotiation
events are logged with both versions for post-incident analysis.

## Technical Contracts

- **Storage format stability:** on-disk SQLite schema changes only via
  idempotent migrations; every migration records `(version, checksum,
  applied_at)` and is safe to re-run after crash mid-apply.
- **WAL health:** checkpoint policy (passive during load, restart after N frames
  or M bytes), WAL-size metric export, and a documented recovery path
  (`PRAGMA integrity_check` → restore from last good snapshot → replay deltas).
- **Embedding dimensions:** dimension is a database-level property negotiated at
  creation; mixed-dimension rows are rejected at write time, never repaired at
  query time.
- **CODEX-blast coordination:** blast index rebuilds are recorded as
  `{ index_version, dim, corpus_hash }` so replicas can detect stale indexes
  without exchanging vectors.
- **MSRV/core features:** core builds on stable Rust with no C toolchain and no
  default network features; plugin features are opt-in and never default.

## Consequences

### Positive

- **Build time collapse for core contributors:** removing the C toolchain and
  P2P stacks from the default build attacks the ~23-minute full build at its
  root; core-only builds and core-only CI jobs become fast and cache-friendly.
- **Independent release cadences:** the C parser fork and the mesh transports
  can ship weekly without forcing a core version bump; core can ship storage
  fixes without re-qualifying networking.
- **Embeddability:** downstream SWAL apps compile a small, dependency-lean core
  (SQLite + sqlite-vec + embeddings + migrations) and add only the sidecars
  they need — frictionless install and replication between apps.
- **Failure isolation:** a parser crash or a network partition degrades to
  `ParserUnavailable` / `TransportDegraded`, never to database corruption; WAL
  health and migration idempotency hold regardless of plugin state.
- **Auditability:** the versioned contract (UDS/IPC/WASM + handshake + typed
  errors) makes every core↔plugin interaction loggable, replayable, and
  testable with a stub sidecar.

### Negative

- **Integration overhead:** every cross-boundary call pays serialization,
  timeout, and version-negotiation cost; naive chatty call patterns (e.g.,
  per-symbol RPC during a large scan) will regress scan throughput until
  batched.
- **Contract maintenance burden:** schema evolution now requires a deprecation
  policy (N-1 minor support), contract tests, and a stub-sidecar harness in CI.
- **Duplicated types:** core and sidecars each own generated copies of contract
  schemas; drift is possible and must be caught by checksum tests, not by
  compiler errors.
- **Debuggability across processes:** stack traces no longer span the boundary;
  incidents require correlated logs (`message_id` join) instead of a single
  backtrace.

### Neutral

- **Workspace layout unchanged in this ADR:** no files are moved by this
  record; extraction of `src/mesh/*` transports and any C fork residue into
  sidecar repositories is sequenced follow-up work tracked in features/issues,
  not in this document.
- **Mesh Phase 1 HTTP behavior preserved:** existing HTTP sync semantics
  documented in `SUBAGENT-OUT/wave23-mesh-design.md` remain valid until the C2
  sidecar contract supersedes them; this ADR changes the packaging, not the
  protocol semantics.
- **Panel UI untouched:** C3 extraction has no storage-semantics impact; it only
  removes presentation constraints from core builds.
- **What would invalidate this decision:** (a) core embedders routinely needing
  in-process parsing or transport (measured: >50% of downstream apps vendoring
  the sidecar back in-process within 6 months); (b) contract overhead exceeding
  20% of p99 query latency for local-only workloads; (c) a security review
  finding that UDS/IPC exposure widens rather than narrows the attack surface
  versus in-process transports.

## Verification

- File-level: `SUBAGENT-OUT/adr-018-plugin-first-boundary.md` exists, exceeds
  80 lines, and contains sections Status, Context, Decision, Structural
  Boundaries (Core vs Plugin), Protocol Contracts, Consequences (Positive,
  Negative, Neutral).
- Mandate fidelity: invariants §1–§3 (C fork external, SWAL network as
  versioned sidecar, distributed-DB logic retained in core) stated verbatim as
  normative requirements.
- Follow-up (out of scope for this file): sidecar extraction issues, contract
  schema crates, stub-sidecar CI harness, and core-only build-time benchmark
  evidencing the reduction from the ~23-minute baseline.
