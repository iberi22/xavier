[local-first][bug] Estabilizar xavier-core: 60 errores (imports no resueltos en sqlite_vec_store + crypto)

## 🎯 Contexto

Tras revertir el PR malformado #532 (que mass-deleted xavier-core) y restaurar el crate vía #535, `xavier-core` no compila: **60 errores**, mayoritariamente `E0433` (unresolved imports). Este crate ya era conocido como "incomplete and non-compiling" (ver commit `fix: remove broken xavier-core from workspace`). Como está en el workspace (`Cargo.toml` member), bloquea `cargo check --workspace`.

## 📋 Los 60 errores, categorizados

### Grupo A — sqlite_vec_store: imports no resueltos (E0433, ~20 errores)
Archivos afectados: `xavier-core/src/sqlite_vec_store/{audit,store_impl,backend_impl,db,graph,schema_impl,search,vector,types}.rs`.
- Importan símbolos que no existen o se movieron. Concretamente líneas `:5`, `:12`, `:37-41`, `:45`, `:52`, `:88-96`, `:122`, `:127`, `:255`, `:378`.
- `store_impl.rs` y `schema_impl.rs` concentran la mayoría.

### Grupo B — crypto/keys.rs (E0433, ~6 errores)
`xavier-core/src/crypto/keys.rs` líneas 33,34,88,89,90 importan símbolos no resueltos.

### Grupo C — otros (hybrid.rs:26, rerank.rs:89, settings/serialization.rs:88-96)
Imports sueltos no resueltos.

### Grupo D — E0425/E0599 (missing symbols, ~17)
Símbolos usados pero no definidos en el crate.

## ✅ Criterio de aceptación

1. `cargo check -p xavier-core` pasa sin errores.
2. `cargo check --workspace` pasa completo.
3. **No** eliminar funcionalidad existente — los errores son de imports/símbolos, hay que resolver las rutas o implementar los símbolos faltantes, no borrar los usos.
4. Si un módulo entero es irrecuperable, documentarlo con `//! TODO: ...` y **stub mínimo compilable** (no `panic!`), preferible a dejarlo roto.
5. `cargo clippy -p xavier-core -- -D warnings` sin warnings nuevos.

## 🔧 Alcance de archivos

- `xavier-core/src/sqlite_vec_store/*.rs` (resolver imports)
- `xavier-core/src/crypto/keys.rs`
- `xavier-core/src/hybrid.rs`, `rerank.rs`, `settings/serialization.rs`
- Posiblemente `xavier-core/src/lib.rs` (re-exports)

## 🔍 Cómo diagnosticar

Ejecutar y leer la salida completa:
```bash
cargo check -p xavier-core 2>&1 | tee /tmp/xc_errors.txt
```
Cada `E0433` dice el símbolo no resuelto y el archivo:línea. El patrón típico: un módulo referencia `crate::X` donde X no está exportado, o `super::Y` donde Y no existe en el padre.

## 🧪 Cómo verificar
```bash
cargo check -p xavier-core
cargo check --workspace
```

## 📎 Contexto histórico
- Commit `3000d349` "extract xavier-core" lo creó.
- Commit "fix: remove broken xavier-core" intentó quitarlo.
- PR #532 lo mass-deleted accidentalmente; #535 lo restauró pero con sus errores internos pre-existentes.
