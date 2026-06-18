# Xavier Governance Vision

## Dual License: MIT + Mesh License

### MIT (Standalone Use)
Xavier core (memory, search, encryption, session management) remains **MIT licensed**.
Anyone can use, modify, and distribute Xavier as a local memory system without restrictions.

### Mesh License (Network participation & Commercial use)
When a node operator activates mesh, Data Commons, or Enterprise features, they agree to the Xavier Mesh License:
1. **Usage Tiers** — Free for individuals and open-source projects. Commercial entities exceeding revenue ($5M) or employee (50) thresholds require a commercial license.
2. **Governance participation** — node earns voting rights proportional to activity + reputation.
3. **Data Commons opt-in** — anonymized telemetry contributes to network health.
4. **XP token rewards** — active participants earn XP for contributing compute, data, or validation.
5. **License enforcement** — mesh license is verified during CLI execution and network handshake.

The Mesh License is a covenant for participation and commercial alignment. If you only use Xavier locally as a standalone tool, you never need it.

## Governance Architecture

### Bicameral DAO (50/50 Split)

```
┌──────────────────────────────────────────────────────────────────┐
│                        XIP Proposal                              │
│  (Xavier Improvement Proposal — any node can submit)             │
└──────────────────────────────────────────────────────────────────┘
                              │
                     Discussion (3 days)
                              │
                   ┌──────────┴──────────┐
                   ▼                     ▼
        ┌─────────────────────┐  ┌─────────────────────┐
        │  Chamber 1: NODES   │  │  Chamber 2: COUNCIL  │
        │  50% voting weight  │  │  50% voting weight   │
        │  Weighted by XP +   │  │  1 member = 1 vote   │
        │  reputation +       │  │  Public identities   │
        │  activity (7d)      │  │  Elected by nodes    │
        │  Anonymous vote     │  │  annually            │
        └────────┬────────────┘  └───────────┬──────────┘
                 │                           │
                 └──────────┬────────────────┘
                            ▼
               Majority in BOTH chambers?
                      /            \
                    YES             NO
                     │               │
              ┌──────▼──────┐  ┌────▼─────┐
              │  APPROVED   │  │ REJECTED │
              │  Timer 48h  │  └──────────┘
              │  → Execute  │
              └─────────────┘
```

### Veto & Override
- **Council Veto** — 66% council vote, only for security/ protocol integrity / decentralization threats
- **Community Override** — 75% community vote can override any council veto

### Voting Weight Formula

```
VotingWeight = (XP_balance × 0.4) + (Reputation_score × 0.3) + (Activity_7d × 0.3)

Where:
- XP_balance = tokens held (capped at 1% of total supply to prevent whales)
- Reputation_score = EigenTrust score from data commons contributions
- Activity_7d = number of mesh interactions in last 7 days (capped at 100)
```

### Quorum
- Minimum 20% of eligible wallets must vote
- If quorum not met in 7 days, proposal automatically fails
- Critical proposals require 33% quorum

## XP Tokenomics

### Earning XP
| Action | XP Reward | Rate Limit |
|--------|-----------|------------|
| Share anonymized telemetry | 1 XP / day | Daily |
| Validate peer data | 2 XP / validation | 10/day |
| Mesh sync contribution | 1 XP / sync | 20/day |
| Run a health-checked node | 5 XP / day | Daily |
| Bug report / security find | 10-50 XP | Per event |
| Vote on XIP | 1 XP / vote | Per proposal |

### Spending XP
| Action | XP Cost |
|--------|---------|
| Submit XIP | 10 XP |
| Premium API rate limit increase | 100 XP |
| Data export (full bundle) | 50 XP |
| Transfer to another node | Free (5% burn) |

### Staking
- Stake 100+ XP for 30+ days → get access to council elections
- Stake 500+ XP for 90+ days → become eligible for council nomination
- Staked XP cannot be spent but earns 0.1% daily interest

## Node Identity Wallet

Each Xavier node generates an Ed25519 keypair at first startup:
- **NodeID** = blake3(ed25519_public_key)
- **Wallet** = derived from NodeID
- **Public Key** = used for mesh handshake encryption
- **Private Key** = never leaves the node (stored encrypted at rest)

Wallet state persists as JSON in the node's config directory.

## Data Commons

### Opt-in Model (default: off)
All telemetry collection is opt-in. Default is `ConsentLevel::None`.

| Consent Level | What gets shared |
|---------------|-----------------|
| None | Nothing |
| Metadata | NodeID hashed, version, uptime (no payload) |
| Anonymized | Metadata + telemetry payloads (CPU, memory, errors), NodeID one-way hashed |
| Full | Everything including query logs for model training (anonymized) |

### EigenTrust Reputation
- Nodes rate each other's data quality
- Reputation is weighted by the rater's own reputation
- Low-reputation nodes are ignored by the mesh
- High-reputation nodes earn more XP per contribution

## Runtime Health Loop

```
┌────────────────────────────────────────────────────────┐
│               Xavier Runtime Loop (continuous)          │
├────────────────────────────────────────────────────────┤
│  Health → Bench → Gap → Experiment → Measure → Merge   │
└────────────────────────────────────────────────────────┘
```

Runs as a background tokio task in the `xavier` binary:
1. **Health Check** (every 60s) — disk, SQLite VACUUM, embedding provider ping, mesh peer connectivity
2. **Benchmark** (every 6h) — recall@k against stored production queries
3. **Gap Analysis** — compare benchmark results to 100% target
4. **Auto-Experiment** — if gap > 2%, generate config tweaks (chunk overlap, RRF weights, policy params)
5. **Validate** — run benchmark again with experiment config
6. **Auto-Merge** — if improvement > 1%, persist config; otherwise rollback

All results stream to the notification system and are visible in the panel UI.

## Mesh Network Architecture (Future)

```
Phase 1 (current)   HTTP REST + Ed25519 auth
Phase 2 (planned)   Iroh/QUIC — NAT traversal, QUIC transport, hole-punching
Phase 3 (planned)   Loro CRDT — conflict-free merge of distributed memory
Phase 4 (future)    Tor/Yggdrasil — anonymous transport option
```

### Sync Protocol
- **Gossip-based** — nodes share manifests, not full payloads
- **Chunk references** — by content hash (blake3), deduplicated
- **Pull-based** — request only what you don't have
- **Encrypted** — all payloads encrypted with peer's public key
- **Anonymized** — telemetry goes through a mixnet

## License Transition Roadmap

1. ✅ **Current**: MIT — everything is MIT
2. ⏳ **Phase 1**: Add Mesh License detection at startup (feature-gate mesh behind license check)
3. ⏳ **Phase 2**: Mesh features require license acceptance; standalone features remain MIT
4. ⏳ **Phase 3**: Smart contract for on-chain governance + XP token
5. 🔮 **Phase 4**: DAO-controlled treasury, grant program, formal council elections

---

*Xavier v0.6.1-beta — Governance Vision Document*
*Last updated: 2026-06-16*
