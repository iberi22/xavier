# SRC — Source Code Reference — xavier

> **Protocol:** GitCore 3.8.0  
> **Updated:** 2026-07-17  
> **Completeness:** structure 100% (update tree when modules change)

## 1. Overview

xavier — [![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

| Field | Value |
|-------|--------|
| Path | `E:\proyectosSWAL\xavier` |
| Stack | see package manifests |
| Protocol | GitCore 3.8.0 |
| Visibility | private (SWAL default) |
| Pro model | SWAL node active (not Stripe) |

## 2. Directory structure

```
xavier/
├── AGENTS.md
├── SRC.md
├── .git-core-protocol-version
├── .gitcore/
│   ├── ARCHITECTURE.md
│   ├── AGENT_INDEX.md
│   ├── features.json
│   └── planning/
│       ├── PLANNING.md
│       └── TASK.md
├── docs/
│   └── SRS/
│       ├── index.md
│       ├── REQUIREMENTS.md
│       └── ARCHITECTURE.md
├── .github/
│   └── workflows.disabled/   # GitHub Actions OFF by default
└── … (project sources)
```

> **Agent task:** replace this tree with the **real** module tree after scan.

## 3. Core components

| Component | Path | Purpose |
|-----------|------|---------|
| Protocol meta | `.gitcore/` | Architecture, features, planning |
| SRS | `docs/SRS/` | Formal requirements |
| Agent rules | `AGENTS.md` | Read order + constraints |
| Sources | *(fill)* | Product / library code |

## 4. Build / run / test

```bash
# Document real commands for this project:
# install / build / test / lint
```

## 5. Environment

| Variable | Purpose | Required |
|----------|---------|----------|
| *(from .env.example)* | | |
| `XAVIER_URL` | Xavier HTTP (default http://127.0.0.1:8006) | for agentic memory |
| `XAVIER_TOKEN` | Auth token | when server enforces auth |
| `XAVIER_DATA_DIR` | Vault + identity + anchor receipts | node identity |
| `XAVIER_NODE_DEVICE_KEY` | Optional device key (WebAuthn PRF hook) | optional |
| `SWAL_POLYGON_RPC_URL` | RPC Polygon (Amoy/mainnet) | anchors live |
| `SWAL_ANCHOR_CONTRACT` | Registry address post-deploy | anchors live |
| `SWAL_ANCHOR_KEY` | Signer key (never commit) | anchors live |
| `SWAL_ANCHOR_DRY_RUN` | `1` default — no broadcast | anchors |
| `SWAL_ANCHOR_BROADCAST` | `1` + `--features dao-evm` → live tx | anchors |

Never commit real secrets.

## 6. SWAL integration

| Concern | Approach |
|---------|----------|
| Pro features | Active SWAL node (`pro_gate` + heartbeat) |
| Multi-instance | `instance_id` · namespaces `swal/{app}/{instance}` |
| Memory | Xavier HTTP/MCP |
| Mesh | edge-mesh data plane (no L1) |
| Identity / login | `src/node_identity/` + `src/polygon_anchor/` — **95%** · `.gitcore/docs/DECENTRALIZED_LOGIN_PROGRESS.md` |
| Payments Pro | **No Stripe** |

### Login / identidad (2026-07-28)

| Module | Path | Notes |
|--------|------|-------|
| Node identity | `src/node_identity/` | BIP39, Shamir, vault, hybrid_pack |
| Polygon anchors | `src/polygon_anchor/` | dry-run default; broadcast `dao-evm` |
| Mesh auth | `src/mesh/{challenge,namespace,pro_gate}.rs` | Fase 1 |
| CLI | `src/cli/commands/node.rs` | `create\|recover\|status\|anchor\|anchor-pack` |

**Siguiente:** ops Amoy deploy · Fase 4 research · UI Maloca WebAuthn.

## 7. Cross-references

| Doc | Path |
|-----|------|
| AGENTS.md | `AGENTS.md` |
| PLANNING | `.gitcore/planning/PLANNING.md` |
| TASK | `.gitcore/planning/TASK.md` |
| features | `.gitcore/features.json` |
| Feature login | `.gitcore/features/FEATURE-feat-decentralized-login.md` (95%) |
| Issues login | `.gitcore/issues/login/PROGRESS.md` |
| Test evidence | `.gitcore/issues/login/TEST_EVIDENCE.md` |
| Session login | `.gitcore/docs/SESSION_LOGIN_2026-07-28.md` |
| SRS index | `docs/SRS/index.md` |
| SRS REQ-008 | `docs/SRS/REQUIREMENTS.md` |
| E2E login | `tests/e2e/decentralized_login_e2e.rs` |
| SWAL roadmap | monorepo `docs/SWAL/README.md` |

---

*Document version: 3.8.0 · Part of GitCore Protocol · Updated 2026-07-28 (login session)*

