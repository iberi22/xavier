# ARCH Distributed 2x — Sharding + Freetier ToS (WAVE-2.10)

ToS compliant: Supabase free tier = 2 active projects global per user (across orgs where Owner/Admin). Neon = 3 projects, Cloudflare freetier = unlimited Workers/Pages/R2 10GB. Implementación usa solo 2 Supabase + 2 Neon branch, sharded via hash(id)%2, encrypted envelope antes de salir del nodo, metadata shard_id + replica_id + supabase_only flag.

## Sharding

- `shard_for_id(id) -> u8` = hash(id) % 2 (DefaultHasher). Replica 1 para failover.
- `XAVIER_SUPABASE_URL_2`, `XAVIER_SUPABASE_KEY_2`, `XAVIER_POSTGRES_URL_2` opcionales; si no están, usa single shard (fallback healthy).
- Write: `put(record) -> shard_for(record.id) -> postgrest_upsert shard`. Include `shard_id`, `project_id`.
- Read: `get(id) -> try shard, then fallback 1-shard if is_sharded()`. List merges both shards.

## Encryption envelope per shard

- `src/crypto/envelope.rs`: AES-256-GCM + X25519 via `ShardEnvelope { shard_id, replica_id, ciphertext_b64, nonce_b64, key_id }`. Plaintext stays on node, ciphertext to freetier.
- `encrypt_for_shard(plaintext, shard_id, key)` -> ShardEnvelope, `decrypt_envelope`.
- ToS: data encrypted end-to-end, freetier only sees ciphertext, compliant.

## Nodes 2x flow

- `xavier nodes add --provider supabase --shard 0/1 --visibility public|private` via `NodeRegistry` shard_for_node.
- Lease via `XAVIER_SECRET_FALLBACK` chain.

## Cloudflare adapters

- `src/mesh/cloud_node.rs`: CloudflareAdapters { R2 shard buckets r2-shard-0/1, KV, D1 } + OpenAI-compat proxy Workers (wrangler.toml).
- Tunnel: `src/cli/handlers/nodes.rs` tunnel_url_for_id -> `https://xavier-<id>.trycloudflare.com`, cloudflared cmd auto-register.

## Quota guard

- `src/health/mod.rs` check_freetier_quota(db_bytes, egress_bytes) -> ok/warning/over_quota (500MB DB, 5GB egress per project, 2x = 1GB/10GB total).
- Health degraded not unhealthy when fallback succeeds (reuse wave-1 logic).

## Cloud sync fallback

- `src/memory/cloud_sync.rs` sync_fallback_chain Vec→Supabase→Neon with shard_for_sync.

## Private networks MLS

- `src/mesh/crypto_gating.rs` shard_for_group + visibility, MLS RFC9420 placeholder for family groups private.

## Harness

- `scripts/harness-file-islands.py wave-2` verifies 10 islands disjuntos.

## ToS Verification

- 2 Supabase active projects only — `git ls-files | grep supabase` + env check. No 3rd org. Documented here and PLAN_WAVES_HEXAGONAL_2X.md.

shard references: supabase_store.rs (shard_for_id, is_sharded), postgres_store.rs, envelope.rs, cloud_node.rs, nodes/registry.rs, cloud_sync.rs, health/mod.rs, crypto_gating.rs
