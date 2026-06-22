# Xavier Codebase-vs-Docs Alignment Audit Report

**Audit Date:** 2026-06-22
**Auditor:** Subagent Jules
**Scope:** `src/**/.rs` vs `docs/`, `.gitcore/`, `.agent/`, root `.md`

---

## 1. Features Implementation Audit

**Summary:**
Xavier has successfully implemented its core RAG engine, hierarchical memory (HORMER), and TGD pipeline as documented. However, there is a significant desync regarding the SurrealDB backend, which remains in architecture diagrams but is absent from the codebase. Additionally, "Cortex" is documented as removed but remains as a stub binary.

**Findings Table:**

| Doc Ref | Code Ref | Status | Severity | Action |
|---|---|---|---|---|
| `docs/ARCHITECTURE.md` (SurrealDB) | `src/memory/` | **Missing** | **HIGH** | Remove from docs or implement driver. |
| `docs/CORTEX.md` (Removed) | `src/bin/cortex.rs` | **Stub Present** | **MEDIUM** | Delete `cortex.rs` or clarify it's a `main.rs` alias. |
| `ARCHITECTURE.md` (`src/storage/`) | `src/memory/store.rs` | **Mismatch** | **MEDIUM** | Update architecture tree; `storage` is now a child of `memory`. |
| `docs/MEMORY_MANAGER.md` | `src/memory/manager/` | **Aligned** | **OK** | Matches 5-priority system logic. |
| `HORMER_IMPL_PLAN.md` | `src/retrieval/` | **Aligned** | **OK** | F1-F6 features are fully merge-verified. |

---

## 2. APIs/Endpoints Audit

**Summary:**
The API surface is split between public REST v1 endpoints and internal "Xavier" coordination endpoints. Many coordination and sync endpoints are functional in the code but completely missing from `docs/API.md`. There is also a mismatch in how memories are listed in V1.

**Findings Table:**

| Documented Endpoint | Code Handler | Status | Severity | Action |
|---|---|---|---|---|
| `GET /v1/memories` | `src/server/v1_api.rs` | **Mismatch** | **HIGH** | Doc says "list memories", code implements "list primary memories". |
| `GET /ready` | `src/adapters/.../routes.rs` | **Missing** | **MEDIUM** | Add missing `/ready` alias to the router (only `/readiness` exists). |
| Undocumented | `/xavier/verify/save` | **Implemented** | **MEDIUM** | Document the internal system verification API. |
| Undocumented | `/xavier/time/metric` | **Implemented** | **MEDIUM** | Document the agentic time-tracking metrics API. |
| Undocumented | `/api/v1/memory/sync/*` | **Implemented** | **MEDIUM** | Document the new P2P chunk-based sync endpoints. |
| `GET /v1/version` | `src/cli/handlers/...` | **Mismatch** | **LOW** | Code uses `version_handler`, docs imply generic metadata. |

---

## 3. Config/Settings Audit

**Summary:**
The migration to a centralized `XavierSettings` struct is largely complete, but the documentation has not caught up with the explosion of provider-specific environment variables. Internal telemetry and billing paths remain undocumented.

**Findings Table:**

| Doc Ref | Code Ref | Status | Severity | Action |
|---|---|---|---|---|
| `docs/CLI.md` (Env Vars) | `src/settings/env.rs` | **Incomplete** | **MEDIUM** | Add `GROQ_API_KEY`, `DEEPSEEK_API_KEY`, etc., to CLI docs. |
| Undocumented | `XAVIER_TELEMETRY_DB_PATH` | **Implemented** | **LOW** | Document for users managing multiple workspaces. |
| `README.md` (Default Port) | `src/cli/config.rs` | **Aligned** | **OK** | Default port 8006 is consistent everywhere. |
| Undocumented | `STRIPE_PRICE_CLOUD` | **Implemented** | **MEDIUM** | Document billing config requirements for Enterprise mode. |

---

## 4. Outdated Decisions / Plans Audit

**Summary:**
Several planning documents describe migration phases (Cortex -> Xavier) that have been completed. These files now serve as historical context but can confuse new developers looking for the current "source of truth."

**Findings Table:**

| Doc Ref | Code Ref | Status | Severity | Action |
|---|---|---|---|---|
| `docs/PLAN_V1.md` | `src/enterprise/` | **Stale** | **LOW** | Archive; enterprise features are already ported. |
| `GOVERNANCE_DAO_PLAN.md` | `src/mesh/` | **Stale** | **MEDIUM** | Code is much further ahead (v0.12 spec) than this plan. |
| `docs/ARCHITECTURE.md` | `hexagonal` | **Aligned** | **OK** | Ports & Adapters structure matches `src/ports/`. |

---

## 5. Name Changes Audit

**Summary:**
Public traits and types have evolved from generic names to domain-specific names, leaving "dead" names in the documentation.

**Findings Table:**

| Doc Ref | Code Ref | Status | Severity | Action |
|---|---|---|---|---|
| `MemoryBackend` (Trait) | `MemoryStore` | **Renamed** | **MEDIUM** | Update all architectural docs to use `MemoryStore`. |
| `EmbeddingPort` (Trait) | `Embedder` | **Renamed** | **MEDIUM** | Update docs to use `Embedder`. |
| Port `8003` | Port `8006` | **Mismatch** | **MEDIUM** | Update `.agent/rules/` which still point to port 8003. |

---

## 6. Documentation Drift (.gitcore/ .agent)

**Summary:**
The `.gitcore` repository is generally accurate regarding the module tree, but `.agent/rules` contain hardcoded paths and database references (SurrealDB) that will cause subagents to hallucinate capabilities.

**Findings Table:**

| Doc Ref | Code Ref | Status | Severity | Action |
|---|---|---|---|---|
| `.agent/rules/rule-0.md` | `SurrealDB` | **STALE** | **HIGH** | Change "Memory store: SurrealDB" to "SQLite-Vec". |
| `.gitcore/SRC.md` | Directory Tree | **OK** | **LOW** | Mostly aligned with `v0.10.x`. |
| `SKILL.md` (agent-ops) | HORMER Policy | **Aligned** | **OK** | Strategy matches `src/retrieval/navigation.rs`. |

---

## Recommendations

1. **SurrealDB Cleanup:** Remove all references to SurrealDB from `docs/ARCHITECTURE.md` and architecture diagrams to reflect the permanent move to SQLite-Vec for the core distribution.
2. **API Documentation Update:** Update `docs/API.md` to include the `xavier/` namespace endpoints (verify, time, events). These are critical for agent-system coordination.
3. **Agent Rules Correction:** Update `.agent/rules/rule-0.md` immediately. Hallucinating SurrealDB as the backend causes significant friction for AI agents performing data operations.
4. **Cortex Binary:** Either remove `src/bin/cortex.rs` or rename it to `xavier-legacy.rs` to avoid confusion with the "Cortex Removed" documentation.
5. **Archive Stale Plans:** Move `PLAN_V1.md`, `MESH_SYNC_PLAN.md`, and `GOVERNANCE_DAO_PLAN.md` to `docs/archive/` or mark them with a "COMPLETED" banner.
