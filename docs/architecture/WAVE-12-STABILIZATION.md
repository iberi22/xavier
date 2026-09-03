# Wave 12 Stabilization Architecture

This document records the architectural stabilization work completed in Wave 12,
covering CPU retry loop mitigations, alert deduplication, embedding provider
fallback mechanisms, and build infrastructure enhancements.

## 1. Problem Statement

During operational deployment, two major cascading failure loops caused
significant operational degradation across system nodes. First, an aggressive
CPU retry loop occurred when the system was configured to use the
`embeddinggemma` model, but the model was missing or unserviceable on the host
Ollama instance. The embedding pipeline re-attempted failed requests
continuously without probing model availability or transitioning to a fallback
backend, causing CPU utilization to spike to near 100%.

Second, `SessionSyncTask` suffered from an unbacked self-call loop that
triggered a severe alert flood. When background session synchronization
encountered a transient error, the task loop immediately re-invoked itself
without exponential backoff or circuit breaking. Each rapid execution attempt
generated system alerts, overwhelming event buffers, inflating memory usage, and
flooding operational log outputs.

## 2. Root Cause Analysis

Root cause analysis identified three primary structural flaws across
configuration, task scheduling, and telemetry subsystems:

1. **Hardcoded Model Configuration in `config/xavier.config.json`**: The default
   configuration explicitly specified `"embedding_model": "embeddinggemma"`. The
   runtime initialized assuming `embeddinggemma` was installed locally,
   lacking dynamic model discovery or fallback probing when host environments
   provided alternate models such as `nomic-embed-text`.
2. **Missing Backoff Mechanism in `session_sync_task.rs`**: Background task
   execution in `session_sync_task.rs` lacked delay, retry limits, or backoff
   schedules on failure. Any error condition instantly triggered a new
   synchronization iteration, creating a high-frequency tight loop.
3. **Unbounded Alert Emission in `alerts.rs`**: The system alert manager in
   `alerts.rs` emitted events directly to the system event bus without
   rate-limiting or deduplication filters. Repeated errors in tight loops
   triggered thousands of duplicate alert events per minute, degrading telemetry
   and UI performance.

## 3. Solutions Applied (Wave 12)

### Issue 12.01: Dynamic Embedding Model Probing and Fallbacks

In Issue 12.01, the embedding pipeline in `src/embedding/mod.rs` was
re-engineered to perform dynamic model probing on startup and prior to request
dispatch. Rather than assuming the configured model is installed, the system
queries the active Ollama provider for available models (such as
`nomic-embed-text`) and dynamically selects an operational candidate, or falls
back gracefully to a degraded mode to eliminate infinite CPU retry loops.

### Issue 12.02: Session Sync Task Exponential Backoff

In Issue 12.02, `src/tasks/session_sync_task.rs` was updated to incorporate an
exponential backoff schedule with circuit-breaking capabilities. On
synchronization errors, task execution delays increase exponentially up to a
maximum threshold, preventing tight re-invocation loops and maintaining CPU
stability under failure conditions.

### Issue 12.03: Alert Deduplication and Suppression Window

In Issue 12.03, an alert deduplication filter was introduced in `src/alerts.rs`
(and associated observability modules). Duplicate alert instances sharing
identical fingerprint signatures are suppressed within a 60-second window,
capping alert emission rates and protecting system memory and UI notification
components from floods.

### Issue 12.04: Configurable Model Resolution

In Issue 12.04, `config/xavier.config.json` and `src/settings/` were
refactored to replace rigid model defaults with flexible runtime resolution
order. Default settings now allow runtime detection and environment overrides
(`XAVIER_EMBEDDING_MODEL`), decoupling the core engine from hardcoded model
names.

### Issue 12.05: Relocation of Cargo Build Target Directory

In Issue 12.05, build infrastructure was updated to set `CARGO_TARGET_DIR` to
`/home/belal/.cargo/xavier-target` on persistent disk storage. Storing
intermediate rustc compilation artifacts on disk rather than temporary
in-memory filesystems (`tmpfs`) resolved recurring compiler out-of-memory
errors and disk exhaustion during workspace builds.

### Issue 12.06: Health Probes and Diagnostic Telemetry

In Issue 12.06, health service probes in `src/app/health_service.rs` and
diagnostic routines in `src/cli/handlers/doctor.rs` were expanded to monitor
system idle CPU usage and alert queue lengths. Diagnostic output now explicitly
reports active embedding provider states and highlights when fallback modes are
active.

### Issue 12.07: RAG Reranking Pipeline Stabilization

In Issue 12.07, the retrieval and reranking logic in `src/retrieval/` was
restructured to maintain vector search accuracy and deterministic scoring.
Complex temporal signal combinations were isolated to ensure search stability
while laying ground for modular temporal scoring extensions.

### Issue 12.08: Memory Store Indexing Alignment

In Issue 12.08, `src/memory/` vector indexing and schema handling were updated to
ensure seamless operations regardless of whether 768-dimensional
(`nomic-embed-text`) or 1024-dimensional embeddings are supplied. Vector
store initialization now validates dimensionality dynamically, maintaining
index integrity with zero pending alerts at idle.

### Issue 12.09: Build Environment Standardization

In Issue 12.09, build scripts and local environment wrappers were aligned across
workspace tools to enforce unified target directory paths and compiler flags.
This standardization prevents target cache invalidation between local
development, background testing, and automated build scripts.

## 4. Architecture Invariants Established

Wave 12 established the following mandatory architectural invariants across the
codebase:

- **Embedding Fallback Chain**:
  `configured model → probed Ollama model → degraded state`.
  Embedding requests must resolve through active provider capabilities,
  automatically adopting probed Ollama models (e.g. `nomic-embed-text`) if the
  configured model is unavailable, and falling back to degraded operation
  rather than spinning CPU retry loops.
- **Alert Deduplication**: `60-second suppression window`. Any alert matching
  the fingerprint of an active alert within 60 seconds is deduplicated at
  source to prevent alert queue floods.
- **Build Target Directory**: `/home/belal/.cargo/xavier-target` on persistent
  disk. All compilation steps must direct target output to persistent disk
  space to ensure sufficient capacity for large workspace compilation
  artifacts.

## 5. Health Baseline After Wave 12

Following the application of Wave 12 stabilization fixes, the operational
baseline for healthy system nodes is established as follows:

- **Expected CPU Usage at Idle**: `<2%` total system CPU utilization.
- **Expected Alert Queue at Idle**: `0` items in the active system alert queue.
- **Embedding Provider Status**: `healthy` with `nomic-embed-text` active and
  operational.

## 6. Open Items / Follow-ups

- **Reranker Integration of Temporal Signals**: Deferred from Issue 12.07.
  Future work will complete the integration of temporal decay signals into the
  RAG reranking pipeline to complement vector similarity and BM25 text scores.
- **CI Configuration for CARGO_TARGET_DIR**: Standardize `CARGO_TARGET_DIR`
  settings in continuous integration workflows to mirror disk-backed target
  paths and optimize artifact caching across CI jobs.
