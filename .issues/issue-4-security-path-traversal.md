## Descripción
Validar path traversal en `project_id` usado en rutas de base de datos.

## Contexto
El project_id se usa directamente en rutas de SQLite. Aunque ya hay sanitización parcial (commit `14898d3`), se necesita validación completa en todos los puntos de entrada.

## Archivos objetivo
- `src/codebase/conversations_db.rs`
- `src/codebase/db.rs`

## Criterios de Aceptación
- [ ] `project_id` validado con regex alfanumérico
- [ ] Rechazar `/`, `..`, `\`, `~` en todos los project_id
- [ ] Tests de integración con project_ids maliciosos
- [ ] `cargo test` pasa
