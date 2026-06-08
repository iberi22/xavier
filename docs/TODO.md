# docs/TODO.md — Pendientes arquitectónicos Xavier

*Updated: 2026-04-25*

---

## Antes de v1.0 — BLOCKERS

| # | Task | Priority | Status |
|---|------|----------|--------|
| 1 | Fix data race en `get_last_sync_result()` — leer todos los campos bajo un lock | 🔴 CRITICAL | ✅ **COMPLETADO** |
| 2 | Wire `SecurityScanPort` en `cli.rs` O eliminarlo | 🔴 CRITICAL | ✅ **COMPLETADO** |
| 3 | Eliminar `app/` services muertos (`todo!()`) | 🟡 MEDIUM | ✅ **COMPLETADO** |
| 4 | Push a GitHub (GROQ_API_KEY secret blockage) | 🔴 CRITICAL | ✅ **COMPLETADO** |

---

## Puertos — decisión a tomar

| Port | Acción | owner |
|------|--------|-------|
| `MemoryQueryPort` | ✅ Mantener — se usa | P0 (listo) |
| `AgentLifecyclePort` | ⚠️ Evaluar — solo un registry por ahora | PENDIENTE |
| `HealthCheckPort` | ❌ Eliminar — overhead, solo HTTP | PENDIENTE |
| `PatternDiscoverPort` | ❌ Eliminar — stub `todo!()` | PENDIENTE |
| `EmbeddingPort` | ❌ Eliminar — stub `todo!()` | PENDIENTE |
| `AgentRuntimePort` | ❌ Eliminar — stub `todo!()` | PENDIENTE |
| `StoragePort` | ❌ Eliminar — stub `todo!()` | PENDIENTE |

---

## v1.0 Release Criteria

- [x] `cargo check --lib` → 0 errors
- [ ] Todos los endpoints SEVIER verificados con curl
- [x] Data race fix commiteado
- [ ] Dead code ports eliminados
- [ ] docs/ADR/ creado y commiteado
- [x] GitHub push exitoso
- [ ] Docker image build + run exitoso
- [ ] Integration tests compilan (ignore=True)

---

## Post v1.0

| Task | Priority | Status |
|------|----------|--------|
| Cortex Enterprise plugin (ADR-004) | 🔴 HIGH | PROPOSED |
| SessionSync state → CliState (ADR-003) | 🔴 HIGH | PENDIENTE |
| Integration tests reales (no scaffolds) | 🟡 MEDIUM | 80% |
| Graceful shutdown en SessionSyncTask | 🟡 MEDIUM | PENDIENTE |
| `estimate_index_lag()` real (no stub ~5min) | 🟡 MEDIUM | PENDIENTE |

---

## Panel UI (Tauri + React + SQLite Backend)

| Task | Priority | Status |
|------|----------|--------|
| Playwright E2E Setup en `panel-ui` | 🔴 HIGH | PENDIENTE |
| E2E Mock Removal & SQLite Integration | 🔴 HIGH | PENDIENTE |
| Generar instaladores nativos via Github Actions | 🟡 MEDIUM | PENDIENTE |
