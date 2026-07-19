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

Never commit real secrets.

## 6. SWAL integration

| Concern | Approach |
|---------|----------|
| Pro features | Active SWAL node |
| Multi-instance | `instance_id` isolation |
| Memory | Xavier HTTP/MCP |
| Mesh | edge-mesh namespaces `swal/{app}/{instance}` |
| Payments Pro | **No Stripe** |

## 7. Cross-references

| Doc | Path |
|-----|------|
| AGENTS.md | `AGENTS.md` |
| PLANNING | `.gitcore/planning/PLANNING.md` |
| TASK | `.gitcore/planning/TASK.md` |
| features | `.gitcore/features.json` |
| SRS index | `docs/SRS/index.md` |
| SWAL roadmap | monorepo `docs/SWAL/README.md` |

---

*Document version: 3.8.0 · Part of GitCore Protocol*

