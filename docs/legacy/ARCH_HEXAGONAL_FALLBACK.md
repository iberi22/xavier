# Xavier Architecture: Hexagonal Ports, Fallback Mechanics & Free-Tier Cloud Strategy

> **GitCore Protocol v3.8** | Feature tracking: [.gitcore/features.json](.gitcore/features.json)
> Last updated: **2026-08-31** | Wave-1 Foundation Architectural Reference

---

## 1. Hexagonal Ports & Adapters Architecture (Ports Map)

Xavier strictly decouples core cognitive domain logic from infrastructure, transport, and persistence using the Hexagonal Architecture (Ports and Adapters) pattern in Rust. All internal interactions rely on asynchronous trait interfaces enforcing thread safety (`Send + Sync`).

```
                    ┌───────────────────────────────────────────────┐
                    │            INBOUND / DRIVING ADAPTERS         │
                    │  (HTTP REST API, MCP Server, CLI Commands)    │
                    └───────────────────────┬───────────────────────┘
                                            │
                                            ▼
                       ┌─────────────────────────────────────────┐
                       │          INBOUND PORTS (Traits)         │
                       │  - MemoryQueryPort                      │
                       │  - MemoryIngestPort                     │
                       └────────────────────┬────────────────────┘
                                            │
                                            ▼
                    ┌───────────────────────────────────────────────┐
                    │               CORE DOMAIN LOGIC               │
                    │  - MemoryRecord, MemoryDocument               │
                    │  - Hybrid BM25 + Vector RRF Fusion            │
                    │  - Clearance Level & PII Redaction            │
                    │  - Belief Graph & Entity Linking              │
                    └───────────────────────┬───────────────────────┘
                                            │
                                            ▼
                       ┌─────────────────────────────────────────┐
                       │         OUTBOUND PORTS (Traits)         │
                       │  - MemoryStore                          │
                       │  - Embedder                             │
                       │  - MeshTransport                        │
                       │  - ProfileVault                         │
                       └────────────────────┬────────────────────┘
                                            │
                                            ▼
                    ┌───────────────────────────────────────────────┐
                    │           OUTBOUND / DRIVEN ADAPTERS          │
                    │ (SQLite+vec, Postgres/Neon, Supabase, Ollama) │
                    └───────────────────────────────────────────────┘
```

### 1.1 Inbound Ports (Primary / Driving)

Inbound ports define the capabilities exposed by the Xavier core domain to external consumers (APIs, CLI, agents, user interfaces).

* **`MemoryQueryPort`** (`src/ports/inbound/memory_port.rs`):
  Async trait (`#[async_trait] pub trait MemoryQueryPort: Send + Sync`) providing high-level memory operations:
  - `search`: Query retrieval with hybrid filtering (`MemoryQueryFilters`).
  - `expand_depth`: Hierarchical graph and memory depth expansion.
  - `add`, `update`, `delete`, `get`, `list`: CRUD lifecycle operations on `MemoryRecord`.
  - `export`: Public and classified memory export.
  - `ls`: Hierarchical directory tree navigation (`NavEntry`).

* **HTTP REST API v1 Adapters** (`src/adapters/inbound/http/` & `src/server/`):
  Axum-based web routes translating REST requests into inbound port invocations.
* **MCP Server Handlers** (`src/server/mcp/`):
  Model Context Protocol tool dispatches (`mem_search`, `mem_context`, `mem_create`, `mem_update`, `mem_delete`).
* **CLI Command Handlers** (`src/cli/handlers/`):
  Terminal handlers (`memory`, `doctor`, `search`, `ingest`).

### 1.2 Outbound Ports (Secondary / Driven)

Outbound ports define required external services, persistence backends, and cryptographic primitives needed by the domain core.

* **`MemoryStore`** (`src/memory/store.rs`):
  Async trait (`#[async_trait] pub trait MemoryStore: Send + Sync`) unifying persistence backends:
  - Methods: `put`, `get`, `update`, `delete`, `list`, `search`, `hybrid_search`, `graph_hops`, `load_workspace_state`, `save_beliefs`.
  - Backends (`MemoryBackend` enum): `Auto`, `Vec` (SQLite + `sqlite-vec`), `Sqlite`, `Postgres` (Neon), `Supabase`, `File`, `Memory`.
* **`Embedder`** (`src/embedding/mod.rs`):
  Async trait (`#[async_trait] pub trait Embedder: Send + Sync`) producing text vector embeddings:
  - Methods: `encode(text: &str) -> Result<Vec<f32>, EmbeddingError>`, `probe_health() -> Result<f64, EmbeddingError>`, `dimension() -> usize`.
* **`MeshTransport`** (`src/mesh/`):
  Transport abstraction for P2P sync, WebRTC signaling, and encrypted chat relay.

---

## 2. Fallback Engine Architecture

Xavier implements multi-tiered fallback mechanics for both text embedding generation and storage persistence to guarantee operational resilience across offline, edge, and cloud environments.

```
                           +------------------------+
                           |  Embedding Request     |
                           +-----------+------------+
                                       |
                                       v
                           +------------------------+
                           | CachedEmbedder (moka)  |
                           +-----------+------------+
                                       | (cache miss)
                                       v
                           +------------------------+
                           | CircuitBreakerEmbedder |
                           +-----------+------------+
                                       | (circuit CLOSED/HALF-OPEN)
                                       v
                           +------------------------+
                           |    FallbackEmbedder    |
                           | Vec<Arc<dyn Embedder>> |
                           +-----------+------------+
                                       |
             +-------------------------+-------------------------+
             |                         |                         |
             v                         v                         v
   +------------------+      +------------------+      +------------------+
   | Tier 1: Local    | ---> | Tier 2: Cloud    | ---> | Tier 3: No-op    |
   | Ollama / GLLM    | Err  | OpenAI/OpenRouter| Err  | Fallback         |
   | (768d / 384d)    |      | (1536d / 3072d)  |      | (0d empty vec)   |
   +------------------+      +------------------+      +------------------+
```

### 2.1 Embedding Fallback Mechanics

The embedding pipeline wraps implementations in `FallbackEmbedder` holding `Vec<Arc<dyn Embedder>>`:

1. **Tier 1: Local Ollama / GLLM**
   - Tries local Ollama endpoints (`http://localhost:11434/api/embed`) using models such as `nomic-embed-text` or `embeddinggemma`.
   - Local GLLM ONNX/ggml embedded execution fallback.
2. **Tier 2: Cloud OpenAI / OpenRouter**
   - Falls back to `https://api.openai.com/v1/embeddings` (or OpenRouter compatible endpoints) using `text-embedding-3-small`.
3. **Tier 3: Noop Embedder**
   - If all active backends fail, returns `NoopEmbedder` (dimension 0, empty vector) while incrementing atomic metric `EMBEDDING_ERROR_COUNT`.

**Reliability Wrappers:**
* **`CircuitBreakerEmbedder`**:
  Tracks consecutive failures (`AtomicU32`). If failures reach `threshold` (default 5), state transitions from `Closed` to `Open`, rejecting immediate calls during `cooldown` (default 60s) before testing via `HalfOpen`.
* **`CachedEmbedder`**:
  Caches query vectors in SQLite/moka LRU cache with configurable TTL and capacity.

### 2.2 MemoryStore Backend Fallback

The `MemoryStore` dispatcher selects storage engines dynamically based on environmental availability:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        MemoryBackend Selection                         │
├────────────────────────────────────────────────────────────────────────┤
│ 1. Vec / Sqlite (SQLite + sqlite-vec): Local primary fast storage      │
│ 2. Postgres / Neon: Distributed SQL storage (usage_metrics, app_data)  │
│ 3. Supabase / PostgREST: Web Crypto encrypted remote sync              │
│ 4. FileMemoryStore / InMemoryMemoryStore: Offline file / test fallback  │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Multi-Cloud Topology & Free-Tier 2x Sharding Strategy

To operate continuously on zero-cost infrastructure, Xavier uses a multi-cloud dual-project sharding architecture across Cloudflare, Supabase, and Neon.

```
                               ┌───────────────────────────────────┐
                               │        Xavier Core Node           │
                               └────────────────┬──────────────────┘
                                                │
                 ┌──────────────────────────────┼──────────────────────────────┐
                 │                              │                              │
                 ▼                              ▼                              ▼
      ┌────────────────────┐         ┌────────────────────┐         ┌────────────────────┐
      │ Cloudflare Edge    │         │ Supabase Dual Tier │         │ Neon Postgres 2x   │
      │ - Workers Proxy    │         │ - Project Alpha    │         │ - Branch Primary   │
      │ - R2 Media / Blobs │         │ - Project Beta     │         │ - Branch Secondary │
      │ - Edge KV Cache    │         │ (2x Free Sharding) │         │ (2x Compute/RAM)   │
      └────────────────────┘         └────────────────────┘         └────────────────────┘
```

### 3.1 Cloudflare Edge Layer (Workers & R2)
* **Workers Proxy:** Proxies API requests, enforces global rate-limiting, and strips geo-location PII headers.
* **R2 Storage:** Object storage for encrypted database snapshots, model weights, and telemetry dumps without egress fees.

### 3.2 Supabase Dual-Project Sharding (2x Free Tier)
* **Dual Project Pairing:** Configures two independent Supabase projects (`app_data_enc` primary and secondary).
* **Deterministic Sharding:** Tenants and workspace IDs are hashed via HMAC-SHA256 (`tenant_id`), routing even hashes to Project Alpha and odd hashes to Project Beta.
* **Capacity Multiplier:** Doubles free-tier database limits (500MB x 2 = 1GB relational storage) and API request quotas while preserving Row Level Security (RLS) policies.

### 3.3 Neon PostgreSQL Branching & Compute Sharding (2x Free Tier)
* **Dual Neon Projects:** Splits operational tables (`usage_metrics`, `billing_records`, `profile_vault_enc`) across two Neon compute endpoints.
* **Scale-to-Zero & Branching:** Leverages Neon instant branching for zero-downtime schema migrations and automatic scale-to-zero during idle periods.

### 3.4 Cryptographic Zero-Trust Engine
All data stored in Supabase and Neon is encrypted on the client before transmission using AES-256-GCM with keys derived via Web Crypto HKDF-SHA256 (`swal-profile-vault-v1` and `swal-data-node`). External cloud providers see only ciphertext hex representations and HMAC-derived tenant identifiers.
