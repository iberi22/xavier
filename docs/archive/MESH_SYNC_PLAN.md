# Mesh + Memory Sync + Cloud Nodes — Implementation Architecture

## Overview

Multi-node mesh for Xavier: sync memory across 1..N private nodes (Android phone + tablet + PC) with Supabase/Neon as cloud relay nodes.

## Phase 1: Memory Sync (`src/memory/sync/`)

New module. Chunk-based LWW sync between peers.

### Structure
```
src/memory/sync/
├── mod.rs          — PeerMemorySync (main API struct)
├── diff.rs         — diff two MemoryStore snapshots → Vec<ChunkDiff>
├── merge.rs        — LWW merge resolver (timestamp-based)
├── push_pull.rs    — push_to_peer(), pull_from_peer(), sync_loop()
└── manifest.rs     — build/reconcile memory manifests
```

### API

```rust
pub struct PeerMemorySync {
    store: Arc<dyn MemoryStore>,
    http_client: reqwest::Client,
    sync_interval: Duration,    // default 300s
}

impl PeerMemorySync {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self;
    
    /// Full sync against a remote peer: pull their manifest, diff, push local changes
    pub async fn sync_with(&self, peer_url: &str) -> Result<SyncSession>;
    
    /// One-shot push: send local chunks newer than last_sync to peer
    pub async fn push_to(&self, peer_url: &str, since: SystemTime) -> Result<SyncSession>;
    
    /// One-shot pull: fetch peer's chunks newer than since
    pub async fn pull_from(&self, peer_url: &str, since: SystemTime) -> Result<Vec<ChunkDiff>>;
    
    /// Background sync loop (runs every sync_interval)
    pub async fn sync_loop(&self, peers: Vec<String>, stop: Arc<AtomicBool>);
    
    /// Heartbeat: check if peer is alive
    pub async fn ping(&self, peer_url: &str) -> bool;
}
```

### ChunkDiff
```rust
pub struct ChunkDiff {
    pub chunk_hash: String,       // sha256 of content
    pub namespace: String,        // "episodic", "semantic", "working", etc
    pub action: DiffAction,
    pub data: Option<Vec<u8>>,    // present for Add/Update
    pub timestamp: SystemTime,
}

pub enum DiffAction { Add, Update, Delete }
```

### Resolution: LWW (Last Writer Wins)
- Each MemoryRecord has a `revision: MemoryRevision { timestamp, node_id }`
- On conflict: record with newer timestamp wins
- If same timestamp: higher node_id (lexicographic) wins
- Simple, sufficient for 1-3 nodes on same LAN

### SyncSession (metrics)
```rust
pub struct SyncSession {
    pub peer_id: String,
    pub chunks_sent: u64,
    pub chunks_received: u64,
    pub conflicts: u64,
    pub duration_ms: u64,
    pub success: bool,
}
```

### Test Plan
1. Sync two empty stores ← empty diff
2. Push 3 chunks to peer, verify chunks received
3. LWW: later timestamp overwrites earlier
4. LWW: same timestamp → higher node_id wins
5. Delete propagation: delete on A appears on B after sync
6. Partial sync: only chunks newer than `since` are transferred
7. Background sync loop runs until stop signal

---

## Phase 2: libp2p Transport (`src/mesh/transport_libp2p.rs` or replace transport.rs)

### What exists
- `transport.rs`: HTTP REST peer (reqwest::Client based, push/pull json)
- Cargo.toml has `libp2p = 0.56` behind `mesh` feature with:
  - `noise` — encrypted handshake
  - `kad` — Kademlia DHT for peer discovery (WAN)
  - `gossipsub` — pub/sub for chunk broadcasts
  - `mdns` — LAN auto-discovery
  - `tcp`, `yamux` — transport + multiplex
  - `identify` — version/capability exchange

### Architecture
```rust
pub struct MeshP2pTransport {
    swarm: Arc<Mutex<Swarm<Behaviour>>>,
    identity: ed25519::Keypair,
    peer_store: Arc<RwLock<PeerRegistry>>,
}

struct Behaviour {
    mdns: mdns::Behaviour,       // LAN auto-detect
    kademlia: kademlia::Behaviour<kad::store::MemoryStore>,  // WAN DHT
    gossipsub: gossipsub::Behaviour,   // broadcast
    identify: identify::Behaviour,
}
```

### Features
- **Mdns**: auto-discover phone ↔ PC on same WiFi
- **Kademlia**: discover tablet via 4G/5G when mdns fails
- **Gossipsub**: broadcast "new chunk available" → peers pull
- **Identify**: each peer announces version, capabilities (supports sync, cloud relay, etc)

### Integration with sync
```rust
impl MeshP2pTransport {
    pub async fn announce_chunk(&self, hash: &str, namespace: &str);
    // → gossipsub publish: {"type": "chunk_available", "hash": "...", "namespace": "..."}
    
    pub async fn request_sync(&self, peer_id: &PeerId);
    // → send direct message to peer: {"type": "sync_request"}
}
```

### Test Plan
1. Two swarms on loopback: mdns discovers both
2. kad put/get provider record
3. gossipsub pub/sub message roundtrip
4. noise handshake establishes encrypted channel
5. Identify protocol reports correct version
6. E2E: transport.sync_with(other_peer) via libp2p

---

## Phase 3: Cloud Nodes (`src/mesh/cloud_adapter.rs`)

### SupabaseAdapter
```rust
pub struct SupabaseAdapter {
    client: reqwest::Client,
    project_url: String,
    anon_key: String,
}

// REST API (free tier: 500MB DB, 2GB bandwidth)
// - Table: `mailbox` (peer_id, message_json, created_at)
// - POST /rest/v1/mailbox — enqueue message for offline peer
// - GET /rest/v1/mailbox?peer_id=eq.{id} — poll for messages when online
// - Supabase Realtime (WebSocket) optional for push notifications
```

### NeonAdapter
```rust
pub struct NeonAdapter {
    pool: deadpool_postgres::Pool,
}

// pgvector for embeddings (free tier: 0.5GB compute)
// - Table: `chunks` with vector(1536) column
// - ANN search via pgvector's <-> operator
```

### Mailbox Pattern (Async Communication)
```
1. Node A wants to message offline Node B
2. A POST → Supabase mailbox table
3. When B comes online, B polls GET /mailbox?peer_id=eq.{B_id}
4. B processes messages, synchronizes
5. B POST → mailbox response
```

### Sync via Cloud
Option A (simple): Both nodes poll Supabase periodically
Option B (real-time): Supabase Realtime WebSocket pushes new messages
Option C (hybrid): Use Supabase as mailbox + Gossipsub when both online

### Configuration
```toml
[xavier.mesh.cloud]
supabase_url = "https://xxxxx.supabase.co"
supabase_anon_key = "eyJ..."
neon_connection_string = "postgres://user:pass@ep-xxxx.us-east-2.aws.neon.tech/xavier"

[xavier.mesh.cloud.free_tier]
max_db_size_mb = 500
max_bandwidth_gb = 2
```

---

## Phase 4: RAG Agentic Multi-Node

### Query Flow
```
User Query
    │
    ▼
┌─────────────────────┐
│  Local QMd Search   │ ← search local index first (fast, ~10ms)
└─────────┬───────────┘
          │ (low confidence?)
          ▼
┌─────────────────────┐
│  Broadcast Query    │ → gossipsub: {"query": "..."}
│  to Peers           │ ← peers respond with top-K results
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  Fuse Results       │ ← ConfidenceCalibration weighting
│  (System 2 scoring) │ ← counter-argument synthesis if contradictions
└─────────┬───────────┘
          │
          ▼
      Final Answer
```

### Fused Scoring
Each peer returns: `Vec<(MemoryRecord, f32 /* confidence */)>`
- Average confidence for records found in multiple peers (boost)
- Penalize records that contradict peers (entropy penalty, already in System2)
- Final: `ConfidenceCalibration.calibrated_confidence`

---

## Phase 5: E2E Validation

### Test: `tests/mesh_e2e.rs`
1. **3-node sim**: start 3 in-process Xavier instances with unique ports
2. **Discovery**: mdns auto-discovers all 3 on loopback
3. **Sync**: Push chunk from node A → all peers via gossipsub
4. **Verify**: Pull from node C shows chunk from A (via B relay)
5. **Resilience**: Kill node B, sync A↔C, restart B, B re-syncs from C
6. **Cloud mailbox**: A writes → Supabase, B comes online → B reads
7. **RAG query**: "find recipe" → A broadcasts → B has result → C has result → fuse
8. **Latency**: LAN < 500ms, WAN < 5s

---

## Implementation Order

| Phase | Module | Est. lines | Est. tests | Priority |
|-------|--------|-----------|-----------|----------|
| 1 | `src/memory/sync/` | ~800 | 15-20 | 🥇 Immediate |
| 2 | `src/mesh/transport_libp2p.rs` | ~600 | 8-12 | 🥈 After sync stable |
| 3 | `src/mesh/cloud_adapter.rs` | ~400 | 6-8 | 🥉 After libp2p |
| 4 | RAG multi-node integration | ~300 | 4-6 | After cloud |
| 5 | E2E test suite | ~200 | 8-10 | Last |

### Keys to free tier viability
- Sync only diffs (not full stores) — 2GB bandwidth is plenty
- Supabase mailbox with TTL 7 days (auto-cleanup via cron)
- Neon pgvector only stores embedding vectors + chunk hashes, not full content
- Typical memory chunk: ~2KB → 500K chunks fit in 500MB
