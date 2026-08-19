# Plan Maestro v1.0 — SWAL Memory Stack

## Arquitectura Final

```
┌──────────────────────────────────────────────────────────────────┐
│                        SWAL MEMORY STACK                         │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  XAVIER (:8006)  — v1.0.0                               │    │
│  │  • Memoria principal (SQLite-vec + gllm embeddings)     │    │
│  │  • CLI / MCP / TUI                                       │    │
│  │  • Security scanner + Belief Graph + Agents              │    │
│  │  • ENTERPRISE: RBAC / Tenancy / API Keys / Audit         │    │
│  │  • Plugin heartbeat → PgHeart                            │    │
│  └──────────┬───────────────────────────────────────────────┘    │
│             │ PGHEART_EMBEDDER_URL=http://localhost:8006/v1      │
│             ▼                                                    │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │  PGHEART (:8080)  — v1.0.0                              │    │
│  │  • Postgres persistente + pgvector                       │    │
│  │  • Supabase REST (cloud)                                 │    │
│  │  • Actor Model (decay/consolidate/reflect)               │    │
│  │  • Jobs scheduler + Prometheus metrics                   │    │
│  │  • RAG-Fusion + LISTEN/NOTIFY                            │    │
│  └──────────────────────────────────────────────────────────┘    │
│                                                                  │
│  CORTEX → DEPRECATED (features migradas a Xavier)                │
└──────────────────────────────────────────────────────────────────┘
```

## Fases

### FASE 0: Seguridad y Roadmap (~2h)
- [x] Verificar showstoppers (mayoría ya resueltos)
- [ ] Actualizar PUBLIC_RELEASE_ROADMAP.md
- [ ] Fix kanban.rs test hardcoded email (`admin@swal.ai` → `test@example.com`)
- [ ] Audit Debug derives en structs con secrets

### FASE 1: Enterprise Features de Cortex a Xavier (~3h)
- [ ] Crear `src/enterprise/` en Xavier con:
  - `mod.rs` — module exports
  - `rbac.rs` — Copy de Cortex (Role, Permission, RoleGuard)
  - `tenancy.rs` — Copy de Cortex (Tenant, Plan, TenantStore)
  - `audit.rs` — Copy de Cortex (AuditEntry, AuditLog, AuditAction)
  - `keys.rs` — Copy de Cortex (ApiKey, ApiKeyStore, ApiKeyType)
  - `rate_limit.rs` — Copy de Cortex (RateLimiter, RateLimitConfig)
- [ ] Wire `enterprise` feature en Cargo.toml
- [ ] Conectar RBAC con auth middleware existente

### FASE 2: PgHeart Fixes (~3h)
- [ ] Fix `db::listener` — NO iniciar listener en modo supabase
- [ ] Fix `list_by_agent` — bug de parsing
- [ ] Agregar /heartbeat endpoint para Xavier plugin
- [ ] Iniciar PgHeart en modo supabase sin errores

### FASE 3: Cortex Deprecación (~1h)
- [ ] Banner deprecación en README.md
- [ ] Instrucciones de migración a Xavier
- [ ] Cerrar issues con tag `migrated-to-xavier`

### FASE 4: Release (~2h)
- [ ] Run `scripts/release-smoke.sh` against live server — all checks PASS
- [ ] Run `scripts/release-smoke.ps1` against live server — all checks PASS
- [ ] Run `scripts/release-smoke.ps1 -RequirePanel` / `XAVIER_REQUIRE_PANEL=1` if panel UI is built
- [ ] Commit + Push Xavier (cambios actuales + enterprise + fixes)
- [ ] Commit + Push PgHeart (fixes + mejoras)
- [ ] GitHub Release Xavier v1.0.0
- [ ] GitHub Release PgHeart v1.0.0
