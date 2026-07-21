---
title: "Bicameral Governance DAO"
description: "Bicameral governance system â€” 50% node operators + 50% Xavier Core Council â€” with weighted voting, veto, XIP lifecycle, and on-chain integration"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A decentralized, bicameral governance framework to coordinate network updates, protocol standards (XIPs), and community standards. The DAO partitions power between active peer-to-peer node operators (50%) and the Xavier Core Council (50%) to ensure democratic yet secure protocol evolutions.

## Architecture & Design
Reputation-based weighted voting gauges node contribution and operational uptime via `ReputationManager`. The Xavier Improvement Proposal (XIP) lifecycle manages proposal registration, secure voting thresholds, and automated quorum calculations. A balance of power is preserved through council vetos and high-percentage community overrule overrides.

## Implementation Paths
- `src/data_commons/governance.rs` (XIP tracking, voting math, and veto triggers)
- `src/data_commons/reputation.rs` (EigenTrust-inspired node reputation metrics)
- `src/data_commons/types.rs` (proposal and voting state schemas)
- `src/mesh/governance.rs` (network-level consensus checks)

## Sub-features
- **Bicameral Separation of Powers:** Equal representation for node operators and the council.
- **Weighted Voting:** Votes are scaled dynamically by node reputation and activity history.
- **XIP Proposal Lifecycle:** Discussion, voting, veto, and execution state machines.
- **Council Veto & Community Override:** A 66% council veto blocks malicious proposals, which can be bypassed by a 75% community supermajority overrule.
- **Quorum Detection:** Intelligent, sliding-scale quorum calculations matching proposal gravity.

## Test References
- Quorum verification and reputation weight calculation tests.
- Veto and overrule consensus simulation unit tests.

## Known Issues & Notes
- On-chain smart contract integration (Solana/Ethereum) is designated as a Phase 2 item (EPIC #115) and does not block local-first product release.

### Functional DAO Proposal Example
Submit a new Xavier Improvement Proposal (XIP) to the bicameral DAO:

```bash
curl -X POST "http://localhost:8006/v1/mesh/governance/proposals" \
  -H "X-Xavier-Token: $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "XIP-12: Standardize Context Bridges",
    "description": "Formally specify the schema for multi-node database bridges.",
    "creator_node_id": "peer-node-998"
  }'
```
