# Sprint Completion: Iroh Mesh, recall@k Eval, and the Auto-Improvement Loop

**Date**: 2026-07-02
**Author**: Xavier AI
**Tags**: [mesh, iroh, quic, retrieval, eval, auto-improvement, rrf, sprint]
**Source Files**: [`src/mesh/iroh_transport.rs`](file:///e:/scripts-python/xavier/src/mesh/iroh_transport.rs), [`src/retrieval/eval.rs`](file:///e:/scripts-python/xavier/src/retrieval/eval.rs), [`src/retrieval/tuner.rs`](file:///e:/scripts-python/xavier/src/retrieval/tuner.rs), [`src/auto_improvement/`](file:///e:/scripts-python/xavier/src/auto_improvement/)

---

## TL;DR
The just-closed sprint landed three opinionated architectural bets: (1) the mesh transport switched from a broken libp2p scaffold to **Iroh QUIC** with automatic NAT traversal; (2) retrieval quality is now measured by a deterministic **recall@k harness** instead of vibes; (3) an **auto-improvement loop** closes the gap between measured and target recall by generating, validating, and conditionally accepting config experiments. Together they move three integration features to their sprint targets and reconcile overall maturity to 74%.

## Context & Motivation
Two structural gaps were stalling the project below 80%. The mesh was non-functional — the libp2p path didn't resolve on the project toolchain, leaving the "sovereign mesh" claim theoretical. And retrieval tuning was a black box: RRF weights existed but no one could say whether changing `rrf_k` or the keyword/vector split actually improved results, because there was no fixed benchmark to regress against.

## The Decisions

### Iroh over libp2p
We quarantined the libp2p code behind a `mesh-legacy` feature and shipped `IrohTransport` mirroring the existing 5-method `MeshTransport` surface (handshake / fetch_manifest / fetch_chunks / push_chunks / share_session) over length-prefixed JSON frames on a QUIC bidirectional stream. Iroh 1.0's relay-assisted hole punching means two Xavier nodes sync without exposing public ports or running bootstrap nodes — the single biggest operational win.

### recall@k eval harness
`retrieval::eval` loads a versioned dataset (`scripts/benchmarks/...`), runs each case's query against the live QmdMemory, and judges a hit by path-substring match against `expected_path`. It rolls up `recall_at_k`, MRR, and `hit_rate` from per-case `CaseResult`s. This gives a reproducible number to tune against.

### Auto-improvement validation loop
`AutoImprovementEngine::run_cycle` runs benchmark → gap detection (target vs current) → experiment generation (concrete `config_overrides`) → optional autonomous validation that *re-measures* and only accepts experiments that beat the baseline. Acceptance is gated on real delta, not on hope.

## Trade-offs
- **Iroh**: we accept a new heavyweight dependency and the N0 public relay preset in exchange for NAT traversal that libp2p never gave us. The wire protocol types (`MeshHandshake`, `MeshManifest`) are reused, so a future transport swap stays cheap.
- **recall@k**: path-substring hit judgement is coarse (it misses semantic duplicates), but it is deterministic and CI-runnable — the right place to start.
- **Auto-improvement**: per-candidate re-measurement requires the weights to actually flip within one process; where they can't, the engine surfaces a recommendation instead of silently mis-applying it.

```mermaid
flowchart LR
    A[Benchmark dataset] --> B[recall@k harness]
    B --> C[RetrievalMetrics]
    C --> D[Gap detection]
    D --> E[Experiments + overrides]
    E --> F[Re-measure]
    F -->|delta > 0| G[Accept]
    F -->|else| H[Recommend only]
    I[Iroh QUIC transport] -.sync.-> A
```
