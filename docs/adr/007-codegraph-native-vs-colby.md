# ADR-007: Code Graph Native-Only Canonical Engine (Colby Sidecar Disabled/Stubbed)

*Status: ACCEPTED | Date: 2026-08-01*

---

## Context

Xavier's system originally designed the Code Graph scanning and parsing capabilities to support an optional, highly advanced external indexing sidecar known as "Colby" (the Colby sidecar). This was intended to be installed on first scan of a workspace via user consent. If declined or if installation failed, Xavier would fallback to its internal native AST-backed parser/indexer.

However, in practice, the external Colby sidecar is currently unavailable, mock-disabled, and completely stubbed out within the codebase (`src/codebase/codegraph_sidecar.rs` returns `available: false` with the message `"Code-graph sidecar is currently mock-disabled"`).

Because documentation continued to describe the Colby sidecar as live and operational, developers and automated LLM agents were frequently wasting time trying to install, configure, or invoke the non-operational Colby sidecar pathway.

---

## Decision

We formally declare the internal native tree-sitter indexer as the canonical code graph engine for Xavier, and document that the Colby sidecar is disabled/stubbed until restored.

Specifically:
1. **Canonical Native Indexer**: The native tree-sitter code graph generator (`xavier code scan`) is the default, primary, and sole operational indexing engine.
2. **Sidecar Status Disclosure**: The Colby sidecar is formally documented as disabled/stubbed out at the runtime level. Any setup prompts or installs are bypassed, directly falling back to the native indexer.
3. **Environment & Flag Stubs**: All Colby-related CLI parameters and environment variables (`XAVIER_CODEGRAPH_INSTALL`, `XAVIER_CODE_GRAPH_NATIVE_ONLY`, `XAVIER_CODEGRAPH_BIN`, `--reprompt-codegraph`) are explicitly documented as ignored stubs or no-ops, to prevent further configuration confusion.
4. **Docs Honesty**: We update the CLI references and ADR files to convey the actual state of the sidecar clearly.

---

## Alternatives Considered

- **Keep existing misleading docs**: Rejected, as it causes recurring developer and agent confusion about whether the sidecar is required or functional.
- **Re-implement Colby live sidecar integration**: Rejected for this release cycle. The current priority is stabilizing and shipping the canonical native-only engine (Ola 10 - Stabilize & Ship).

---

## Consequences

**Positive:**
- **Zero Confusion**: Eliminates wasting time on non-functional features or paths.
- **Improved Transparency**: Accurately aligns user expectations and CLI documentation with actual codebase execution paths and the `available: false` runtime status.
- **Unified Native Performance**: Strengthens and prioritizes testing and optimization of the native AST parser.

**Negative:**
- Advanced features exclusive to the external Colby indexing engine remain inactive or unimplemented until a future release restores live sidecar capabilities.
