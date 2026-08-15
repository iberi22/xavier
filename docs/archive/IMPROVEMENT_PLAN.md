# Plan de Mejora: Xavier v0.6.1-beta

**Basado en Code Review Report** — 4 Junio 2026
**Score Actual:** B+ → **Objetivo: A**

---

## Fases de Implementación

### FASE 1 — 🔴 Correcciones Críticas (hoy)

| # | Tarea | Archivos | Impacto | Estado |
|---|-------|----------|---------|--------|
| 1.1 | Reemplazar 22 `unwrap()` en producción con error handling | Varios (routes.rs, provider.rs, router.rs, rate_limit.rs, system1.rs) | Previene panics en runtime | ⏳ |
| 1.2 | Documentar invariants de los 2 bloques `unsafe` | crypto.rs, mod.rs(mod.rs:398) | Safety guarantees | ⏳ |
| 1.3 | Limpiar 7 items de dead code | date.rs, state.rs | Binary más limpio | ⏳ |
| 1.4 | Reemplazar `#[allow(clippy::needless_range_loop)]` | tool_alias.rs | Calidad de código | ⏳ |

### FASE 2 — 🟡 Refactors Medios (semanal)

| # | Tarea | Archivos | Impacto | Estado |
|---|-------|----------|---------|--------|
| 2.1 | Dividir `cli/server.rs` (2,703 líneas) | cli/server.rs | Mantenibilidad | ⏳ |
| 2.2 | Dividir `mcp_server.rs` (2,012 líneas) | server/mcp_server.rs | Testeabilidad | ⏳ |
| 2.3 | Refactor `workspace.rs` (1,552 líneas) | workspace.rs | Mantenibilidad | ⏳ |
| 2.4 | Reemplazar `#[allow(clippy::too_many_arguments)]` | conversations_db.rs, db.rs | Clean code | ⏳ |
| 2.5 | Migrar dependencia `bincode` (unmaintained) | Cargo.toml | Seguridad | ⏳ |

### FASE 3 — 🟢 Mejoras (mensual)

| # | Tarea | Archivos | Impacto | Estado |
|---|-------|----------|---------|--------|
| 3.1 | Agregar tests de integración para mega-módulos | tests/ | Cobertura | ⏳ |
| 3.2 | Validación de path traversal en project_id | conversations_db.rs, db.rs | Seguridad | ⏳ |
| 3.3 | Implementar cron-validator en pipeline CI | .github/workflows/ | Automatización | ⏳ |

---

## Detalle FASE 1 — Correcciones Críticas

### 1.1 Reemplazar unwrap() en producción

**Archivos objetivo:**

#### `src/api/routes.rs` (líneas 551, 582, 612)
```rust
// ANTES
.value(&some_data.unwrap())
// DESPUÉS
.value(&some_data.map_err(|e| AppError::Internal(format!("...: {}", e)))?)
```

#### `src/agents/provider.rs` (líneas 930-931)
```rust
// ANTES
let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
let addr = listener.local_addr().unwrap();
// DESPUÉS
let listener = TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr()?;
```

#### `src/agents/provider.rs` (línea 961)
```rust
// ANTES
let err_msg = format!("{:?}", result.err().unwrap());
// DESPUÉS
let err_msg = match result.err() {
    Some(e) => format!("{:?}", e),
    None => "Unknown error".to_string(),
};
```

#### `src/server/router.rs` (línea 631)
```rust
// ANTES
let policy: RoutingPolicy = serde_json::from_str(policy_json).unwrap();
// DESPUÉS
let policy: RoutingPolicy = serde_json::from_str(policy_json)
    .map_err(|e| AppError::ConfigError(format!("invalid routing policy: {}", e)))?;
```

#### `src/memory/rate_limit.rs` (línea 29)
```rust
// ANTES
ConnectionManager::global().connect(project_id, ".").unwrap();
// DESPUÉS
ConnectionManager::global().connect(project_id, ".")
    .map_err(|e| warn!("Failed to connect: {}", e))?;
```

#### `src/server/system1.rs` (líneas 672, 683)
Revisar contexto y aplicar pattern similar.

### 1.2 Documentar bloques unsafe

#### `src/crypto.rs:17`
```rust
// ANTES
unsafe { String::from_utf8_unchecked(result) }
// DESPUÉS — con safety comment
// SAFETY: result siempre contiene UTF-8 válido porque proviene de
// [fuente específica] que garantiza codificación UTF-8.
// Si [condición] cambia, reemplazar con from_utf8(result).unwrap()
unsafe { String::from_utf8_unchecked(result) }
```

#### `src/.../mod.rs:398`
Revisar y documentar invariants.

### 1.3 Limpiar dead code

#### `src/cli/state.rs` (líneas 29-49)
Remover las 6 variables/funciones con `#[allow(dead_code)]` o eliminar el attribute si son usadas.

#### `src/date.rs:217`
Ídem.

### 1.4 Reemplazar range loops

#### `src/tool_alias.rs` (líneas 78, 82)
```rust
// ANTES
for i in 0..items.len() {
    process(&items[i]);
}
// DESPUÉS
for item in &items {
    process(item);
}
```

---

## Testing Plan

### Por cada fix:
1. `cargo check` — compilación
2. `cargo clippy -- -D warnings` — sin nuevas warnings
3. `cargo test` — tests existentes pasan

### Tests adicionales a crear:
- Tests de error handling para rutas que antes tenían unwrap
- Tests de integración para server/cli (simulando entradas inválidas)

---

## Criterios de Éxito

- [ ] **0** `unwrap()` en código de producción (no test)
- [ ] **0** `unsafe` sin safety comments
- [ ] **0** `#[allow(dead_code)]`
- [ ] **0** `#[allow(clippy::*)]`
- [ ] **0** archivos >1500 líneas
- [ ] **cargo clippy -- -D warnings** pasa limpio
- [ ] **cargo test** pasa completo
