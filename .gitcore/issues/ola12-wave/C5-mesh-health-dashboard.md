# C5: Mesh health dashboard endpoint

## Problem

No centralized view of mesh status. The health endpoint (`/health`) shows
mesh maturity percentages but not operational details (peer latencies,
sync state, bandwidth).

## Solution

Add `GET /v1/mesh/health` endpoint returning detailed mesh operational status.

### Response schema

```json
{
  "status": "healthy",
  "peers": [
    {
      "id": "xavier-node-abc",
      "address": "/ip4/192.168.1.10/tcp/4001",
      "latency_ms": 45,
      "last_seen": "2026-07-31T22:00:00Z",
      "sync_state": "synced",
      "version": "0.12.0"
    }
  ],
  "maturity": {
    "http_transport": 100,
    "acl": 90,
    "libp2p": 10,
    "onchain_gov": 5,
    "tokenomics": 40
  },
  "bandwidth": {
    "bytes_sent": 1024000,
    "bytes_received": 2048000,
    "messages_per_sec": 12.5
  }
}
```

### Steps

1. Create `src/mesh/dashboard.rs` with `MeshDashboard` struct
2. Implement `get_status()` that aggregates peer info + maturity + bandwidth
3. Add route `GET /v1/mesh/health` in `src/server/v1_api.rs`
4. Add auth check (requires valid Xavier token)
5. Unit test for status aggregation

## Acceptance

- [ ] `GET /v1/mesh/health` returns JSON with peers, maturity, bandwidth
- [ ] Response matches schema above
- [ ] Auth required (401 without token)
- [ ] Unit test passes
- [ ] Existing v1_api tests pass

## Files

- `src/mesh/dashboard.rs` (new)
- `src/server/v1_api.rs` (modify)
