# FEATURE-070: Cloudflare Vectorize and Workers AI Edge Persistence Adapter Spec

**Status:** `approved` | **Tier:** SWAL Partner Cloud ($9/mo) | **Last Updated:** 2026-03-31

## Overview

SWAL Partner Cloud ($9/mo subscription) offers serverless edge persistence for Xavier cognitive memory layers. The edge persistence tier utilizes Cloudflare serverless edge infrastructure:
- **Cloudflare Vectorize:** High-performance vector database for embedding indexing and similarity search.
- **Cloudflare Workers AI:** Serverless embedding generation (`@cf/baai/bge-large-en-v1.5` / `@cf/baai/bge-base-en-v1.5`).
- **Cloudflare D1:** Serverless SQL database for storing encrypted memory record metadata (`memories_enc`).
- **Cloudflare Workers Proxy:** Edge API gateway orchestrating zero-knowledge authentication, vector search, and record synchronization.

Xavier integrates with this tier via `CloudBackendType::CloudflareEdge` in `src/memory/cloud_sync.rs`, interfacing with `CloudMemorySync` and postgREST/REST endpoints.

---

## Architecture & Zero-Knowledge Guarantees

```
┌─────────────────────────────────────────────────────────┐
│                    Xavier Edge Node                     │
│  (Local Memory Store + AES-256-GCM Envelope Encryption) │
└────────────────────────────┬────────────────────────────┘
                             │ HTTPS / TLS 1.3
                             │ Payload: Encrypted Records + DEKs + Embeddings
                             ▼
┌─────────────────────────────────────────────────────────┐
│              SWAL Cloudflare Worker Proxy               │
│               (Partner Edge Gateway)                    │
└───────┬────────────────────┬────────────────────┬───────┘
        │                    │                    │
        ▼                    ▼                    ▼
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│  Workers AI  │     │  Vectorize   │     │ Cloudflare D1│
│ (Embeddings) │     │(Vector Index)│     │(memories_enc)│
└──────────────┘     └──────────────┘     └──────────────┘
```

### Zero-Knowledge Encryption
1. **Client-Side Encryption:** All memory record content (`content_enc`) and user metadata (`metadata_enc`) are encrypted using AES-256-GCM before transmission.
2. **Key Protection:** Data Encryption Keys (DEKs) are wrapped with Key Encryption Keys (KEKs) derived locally on the Xavier node.
3. **No Plaintext Leaks:** Neither the Cloudflare Worker proxy, Cloudflare D1, nor Cloudflare Vectorize ever receives unencrypted plaintext content or user secrets.
4. **Vector Privacy:** Vectorize holds only anonymous embedding vectors and non-sensitive structural metadata (record `id` and `workspace_id` namespace filters).

---

## Data Models & D1 Schema

### Cloudflare D1 Table Schema (`memories_enc`)

```sql
CREATE TABLE IF NOT EXISTS memories_enc (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    path TEXT NOT NULL,
    content_enc TEXT NOT NULL,
    metadata_enc TEXT NOT NULL,
    encrypted_dek TEXT,
    content_iv TEXT,
    metadata_iv TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1,
    deleted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_memories_enc_workspace_updated
ON memories_enc (workspace_id, updated_at);
```

### Vectorize Index Configuration

- **Index Name:** `swal-memory-vectors`
- **Dimensions:** 768 (or model default matching Workers AI embedding output)
- **Metric:** `cosine`
- **Metadata Indexing:** `workspace_id` (string filter)

---

## REST API Contracts (Worker Proxy Gateway)

All requests require HTTP Header: `Authorization: Bearer <SWAL_PARTNER_JWT>`.

### 1. Batch Push Endpoint (`POST /v1/sync/push`)

Pushes encrypted memory record deltas and embedding vectors from Xavier to Cloudflare D1 and Vectorize.

#### Request Payload (`CloudflareEdgeSyncPayload`)

```json
{
  "records": [
    {
      "id": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON",
      "workspace_id": "workspace_default",
      "path": "memories/episodic/001.md",
      "content_enc": "a1b2c3d4e5f6...[AES-256-GCM hex/base64]",
      "metadata_enc": "f6e5d4c3b2a1...[AES-256-GCM hex/base64]",
      "encrypted_dek": "0123456789abcdef...[Encrypted DEK hex]",
      "content_iv": "a1b2c3d4e5f6...[IV hex]",
      "metadata_iv": "f6e5d4c3b2a1...[IV hex]",
      "created_at": "2026-03-31T12:00:00Z",
      "updated_at": "2026-03-31T12:00:00Z",
      "revision": 1
    }
  ],
  "vectors": [
    {
      "id": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON",
      "values": [0.0123, -0.0456, 0.0789, "... [768 dimensions]"],
      "namespace": "workspace_default",
      "metadata": {
        "workspace_id": "workspace_default"
      }
    }
  ],
  "node_id": "xavier-node-alpha"
}
```

#### Response (`200 OK`)

```json
{
  "status": "success",
  "pushed_records": 1,
  "pushed_vectors": 1,
  "processed_at": "2026-03-31T12:00:01Z"
}
```

---

### 2. Incremental Pull Endpoint (`GET /v1/sync/pull`)

Pull encrypted memory records and vectors modified since `since` timestamp.

#### Query Parameters
- `workspace_id` (string, required)
- `since` (ISO 8601 string, optional)
- `limit` (integer, default: 100)
- `cursor` (string, optional)

#### Response Payload (`CloudflareEdgePullResponse`)

```json
{
  "workspace_id": "workspace_default",
  "records": [
    {
      "id": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON",
      "workspace_id": "workspace_default",
      "path": "memories/episodic/001.md",
      "content_enc": "a1b2c3d4e5f6...[AES-256-GCM hex/base64]",
      "metadata_enc": "f6e5d4c3b2a1...[AES-256-GCM hex/base64]",
      "encrypted_dek": "0123456789abcdef...",
      "content_iv": "a1b2c3d4e5f6...",
      "metadata_iv": "f6e5d4c3b2a1...",
      "created_at": "2026-03-31T12:00:00Z",
      "updated_at": "2026-03-31T12:00:00Z",
      "revision": 1
    }
  ],
  "vectors": [
    {
      "id": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON",
      "values": [0.0123, -0.0456, 0.0789],
      "namespace": "workspace_default",
      "metadata": {
        "workspace_id": "workspace_default"
      }
    }
  ],
  "next_cursor": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON"
}
```

---

### 3. Vector Similarity Search Endpoint (`POST /v1/search/vector`)

Performs nearest-neighbor vector search over Cloudflare Vectorize.

#### Request Payload

```json
{
  "workspace_id": "workspace_default",
  "vector": [0.0123, -0.0456, 0.0789],
  "top_k": 10,
  "return_records": true
}
```

#### Response Payload

```json
{
  "matches": [
    {
      "id": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON",
      "score": 0.9241,
      "record": {
        "id": "rec_01J9X8Y7Z6W5V4U3T2S1R0QPON",
        "workspace_id": "workspace_default",
        "path": "memories/episodic/001.md",
        "content_enc": "a1b2c3d4e5f6...",
        "metadata_enc": "f6e5d4c3b2a1...",
        "encrypted_dek": "0123456789abcdef...",
        "content_iv": "a1b2c3d4e5f6...",
        "metadata_iv": "f6e5d4c3b2a1...",
        "created_at": "2026-03-31T12:00:00Z",
        "updated_at": "2026-03-31T12:00:00Z",
        "revision": 1
      }
    }
  ]
}
```

---

## SWAL Config Integration

To enable Cloudflare Vectorize Edge Persistence on Xavier:

```json
{
  "cloud": {
    "backend_type": "cloudflare_edge",
    "edge_url": "https://edge.swal.cloud",
    "partner_token": "swal_partner_live_...",
    "auto_sync_interval_secs": 300,
    "batch_size": 100
  }
}
```
