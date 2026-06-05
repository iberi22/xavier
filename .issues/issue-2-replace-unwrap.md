## Descripción
Reemplazar ~20 `unwrap()` en código de producción con error handling apropiado.

## Archivos objetivo

| Archivo | Líneas | Contexto |
|---------|--------|----------|
| `src/api/routes.rs` | 551, 582, 612 | `.unwrap()` en handlers HTTP |
| `src/agents/provider.rs` | 930-931, 961 | TcpListener, local_addr, err_msg |
| `src/server/router.rs` | 631 | serde_json parsing |
| `src/memory/rate_limit.rs` | 29 | ConnectionManager::connect |
| `src/server/system1.rs` | 672, 683 | Revisar contexto |

## Patrón a usar

```rust
// ANTES
let value = some_result.unwrap();

// DESPUÉS
let value = some_result.map_err(|e| {
    tracing::log::warn!("[component] failed to ...: {}", e);
    AppError::Internal(format!("...: {}", e))
})?;
```

## Criterios de Aceptación
- [ ] 0 `unwrap()` en código de producción (no test)
- [ ] `cargo check` pasa limpio
- [ ] `cargo clippy` no da nuevas warnings
