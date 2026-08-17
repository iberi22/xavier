# SRC — Source Code Reference — xavier

> **Protocol:** GitCore 3.8.0  
> **Updated:** 2026-08-16
> **Completeness:** structure 100%

## 1. Overview

xavier — [![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](https://www.gnu.org/licenses/agpl-3.0.html)

| Field | Value |
|-------|--------|
| Path | `apps/xavier` |
| Stack | Rust (Tokio, Axum, SQLite, sqlite-vec), TypeScript (React, panel-ui) |
| Protocol | GitCore 3.8.0 |
| Visibility | private (SWAL default) |
| Pro model | SWAL node active (`pro_gate.rs`, no Stripe) |

## 2. Directory structure

```
xavier/
├── AGENTS.md
├── CLA.md
├── CONTRIBUTING.md
├── LICENSE
├── README.md
├── SECURITY.md
├── SRC.md
├── Cargo.toml
├── .git-core-protocol-version
├── .gitcore/
│   ├── AGENT_INDEX.md
│   ├── MANIFEST.json
│   ├── docs/
│   │   └── SWAL_GOAL.md
│   ├── features.json
│   └── features-detailed.json
├── docs/
│   ├── ARCHITECTURE/
│   ├── CLI.md
│   ├── OPERATIONS.md
│   ├── ROADMAP.md
│   ├── SECURITY.md
│   └── SRS/
│       ├── index.md
│       ├── REQUIREMENTS.md
│       └── ARCHITECTURE.md
├── src/
│   ├── adapters/
│   ├── agents/
│   ├── app/
│   ├── codebase/
│   ├── coordination/
│   ├── crypto/
│   ├── data_commons/
│   ├── domain/
│   ├── embedding/
│   ├── governance/
│   ├── health/
│   ├── memory/
│   ├── mesh/
│   ├── node_identity/
│   ├── nodes/
│   ├── notifications/
│   ├── observability/
│   ├── polygon_anchor/
│   ├── retrieval/
│   ├── scheduler/
│   ├── search/
│   ├── security/
│   ├── server/
│   ├── session/
│   ├── storage/
│   └── telegram/
├── xavier-core/
├── code-graph/
├── panel-ui/
├── parsers/
├── public/
├── scripts/
└── tests/
```

## 3. Core components

| Component | Path | Purpose |
|-----------|------|---------|
| Protocol meta | `.gitcore/` | Architecture, features, planning, SWAL GOAL |
| SRS | `docs/SRS/` | Formal IEEE-830 requirements |
| Agent rules | `AGENTS.md` | Read order, conventions, SWAL integration block |
| Domain logic | `src/` | Hexagonal core, memory, mesh, HTTP/MCP servers |
| Core crate | `xavier-core/` | Low-level vector storage & embedding primitives |
| Code graph | `code-graph/` | AST indexing sidecar & symbol query engine |
| Web UI | `panel-ui/` | React dashboard and Maloca portal components |

## 4. Build / run / test

```bash
# Build binary
cargo build --release

# Run unit and integration tests
cargo test --workspace

# Lint code
cargo clippy --all-targets -- -D warnings

# Verify local audit
python3 /home/jules/self_created_tools/swal_docs_audit_local.py
```

## 5. Environment

| Variable | Purpose | Required |
|----------|---------|----------|
| `XAVIER_URL` | Xavier HTTP (default http://127.0.0.1:8006) | for agentic memory |
| `XAVIER_TOKEN` | Auth token | when server enforces auth |
| `XAVIER_DATA_DIR` | Vault + identity + anchor receipts | node identity |
| `XAVIER_NODE_DEVICE_KEY` | Device key for node auth | optional |
| `SWAL_POLYGON_RPC_URL` | RPC Polygon (Amoy/mainnet) | anchors live |
| `SWAL_ANCHOR_CONTRACT` | Registry address post-deploy | anchors live |
| `SWAL_ANCHOR_KEY` | Signer key | anchors live |
| `SWAL_ANCHOR_DRY_RUN` | `1` default — no broadcast | anchors |
| `SWAL_ANCHOR_BROADCAST` | `1` + `--features dao-evm` → live tx | anchors |

Never commit real secrets.

## 6. SWAL integration

| Concern | Approach |
|---------|----------|
| Pro features | Active SWAL node (`pro_gate.rs` + heartbeat) |
| Multi-instance | `instance_id` · namespaces `swal/{app}/{instance}` |
| Memory | Xavier HTTP/MCP |
| Mesh | Xavier Mesh P2P / Iroh transport |
| Identity / login | `src/node_identity/` + `src/polygon_anchor/` |
| Payments Pro | **No Stripe** |

## 7. Cross-references

| Doc | Path |
|-----|------|
| AGENTS.md | `AGENTS.md` |
| SWAL GOAL | `.gitcore/docs/SWAL_GOAL.md` |
| features | `.gitcore/features.json` |
| SRS index | `docs/SRS/index.md` |
| SRS REQ-008 | `docs/SRS/REQUIREMENTS.md` |
| CLA | `CLA.md` |

---

*Document version: 3.8.0 · Part of GitCore Protocol · Updated 2026-08-16*
