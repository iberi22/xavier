## Descripción
Dividir los archivos >1000 líneas para mejorar mantenibilidad y testeabilidad.

## Archivos a refactorizar

### Prioridad 🔴
1. **`src/cli/server.rs`** (2,703 líneas) — Extraer handlers CLI, HTTP setup y WebSocket handlers
2. **`src/server/mcp_server.rs`** (2,012 líneas) — Separar tool implementations en archivos por dominio

### Prioridad 🟠
3. **`src/workspace.rs`** (1,552 líneas) — Dividir en módulos (workspace struct, file operations, templates, state)
4. **`src/server/http.rs`** (1,267 líneas) — Separar route handlers por prefijo (/v1/, /api/, /health/)

### Prioridad 🟡
5. **`src/retrieval/gating.rs`** (1,100 líneas) — Extraer scoring + fusion (RRF) a archivo separado

## Plan Detallado
Ver `docs/REFACTOR_PLAN.md` para el diseño exacto de cada división.

## Criterios de Aceptación
- [ ] `src/cli/server.rs` < 500 líneas
- [ ] `src/server/mcp_server.rs` < 500 líneas
- [ ] `cargo check` pasa limpio
- [ ] `cargo clippy -- -D warnings` pasa limpio
- [ ] No se modifican archivos de test
