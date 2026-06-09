⚠️ **Build falló** — Errores de compilación en los handlers CLI:

```
error[E0432]: unresolved import `xavier::billing`
  --> src/cli/handlers/billing.rs:9:13

error[E0433]: cannot find `setup` in `handlers`
  --> src/cli/commands/mod.rs:151:53

error[E0603]: function `gather_system_info` is private
  --> src/cli/handlers/headless_api.rs:46:44

error[E0433]: cannot find `OpenAiEmbedder` in `openai`
  --> src/cli/handlers/tests.rs:62:59

error[E0433]: cannot find module or crate `num_cpus`
  --> src/cli/handlers/system_scan.rs:409:15

error[E0277]: the trait bound `FromFn<...>: Service<...>` is not satisfied
  --> src/cli/tests.rs:151, 172, 194, 216
```

**Acciones requeridas:**
1. Hacer público `gather_system_info` en `system_scan.rs`
2. Crear/Exportar el módulo `billing`
3. Crear/Exportar el módulo `setup`
4. Agregar dependencia `num_cpus` al Cargo.toml
5. Arreglar firma de `auth_middleware` en tests (axum middleware)
6. Exportar `OpenAiEmbedder` o ajustar test

Por favor, corrige estos errores y actualiza el PR.