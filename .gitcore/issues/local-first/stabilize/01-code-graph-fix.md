[local-first][code-graph][bug] Estabilizar code-graph: 31 errores de compilación (módulos inexistentes + plugin system incompleto)

## 🎯 Contexto

Tras integrar los 12 PRs de la Ola 1 + revertir el malformado #532 + recuperar `xavier-core` (#535), el workspace NO compila por `code-graph` con **31 errores**. El crate principal `xavier` y `codegraph-types` compilan bien; el problema está aislado en `code-graph`.

Esto es trabajo del plugin system a medio integrar (commits 485-492 + PR #508 que no completó los stubs).

## 📋 Los 31 errores, categorizados

### Grupo A — Módulos inexistentes (E0583, 4 errores)
`code-graph/src/lib.rs` y `code-graph/src/indexer/mod.rs` declaran `pub mod` de archivos que **no existen**:
- `code-graph/src/lib.rs:11` → `pub mod impact;` (archivo `impact.rs` **nunca existió**)
- `code-graph/src/lib.rs:13` → `pub mod mcp;` (existió en `eee2fcc6`, borrado)
- `code-graph/src/indexer/mod.rs:9` → `pub mod watcher;` (existió en `eee2fcc6`, borrado; ver issue #466)
- `code-graph/src/indexer/mod.rs:25` → `pub mod call_resolution;` (existió en `eee2fcc6`, borrado; ver issue #468)

**Fix**: o bien (a) crear los módulos/stubs mínimos que compile, o (b) **comentar/quitar las declaraciones `pub mod`** y todo código que las use. Recomendado: opción (b) para `impact` (zombie) y stubs vacíos compilables para `mcp`/`watcher`/`call_resolution` referenciando sus issues.

### Grupo B — Bug tipos indexer (E0424/E0277/E0599, ~9 errores)
`code-graph/src/indexer/mod.rs`:
- `fn collect_files(root: &Path)` en línea 169 **no tiene `&self`** pero línea 209 usa `self.plugin_host.discovery()`. **Fix**: cambiar firma a `fn collect_files(&self, root: &Path)`.
- Esto causa errores en cascada (`Vec<Path>` not Sized en líneas 53,60,68,76,81,93,108) — todos se resuelven al añadir `&self`.
- Línea 65: `self.db.get_all_file_metadata()` — método inexistente en `CodeGraphDB`.
- Línea 100: `self.db.batch_delete_file_data()` — método inexistente.
- Línea 150: `db.batch_upsert_file_metadata()` — método inexistente.
**Fix**: implementar estos 3 métodos en `code-graph/src/db/mod.rs` (operaciones sobre la tabla `file_metadata` si existe, o crear la tabla).

### Grupo C — Plugin system incompleto (E0432/E0599/E0063/E0277, ~18 errores)
- `code-graph/src/api/plugin_routes.rs:10` importa `LanguageDiscovery`, `PluginHealthMonitor` de `crate::plugin` pero **no están exportados ahí** (están en `plugin::health` y re-exportados en crate root).
- `code-graph/src/plugin/manager.rs:117` inicializa `PluginConfig` sin campos `name` y `extensions`.
- `code-graph/src/plugin/manager.rs:158` inicializa `PluginDescriptor` sin campo `extensions`.
- `code-graph/src/plugin/manager.rs` **no implementa el trait `LanguageDiscovery`** (requerido por `plugin_host.rs:70`).
- `PluginManager` no tiene métodos `health()`, `all_plugin_names()`, `record_success()`, `record_failure()`.
- `code-graph/src/plugin/engine.rs:112` accede a `response.results` pero `PluginResponse` tiene campo `symbols` (no `results`).
**Fix**: alinear `manager.rs` con el trait y structs definidos en `types.rs` y `plugin/health.rs`. Ver `code-graph/src/types.rs:318 LanguageDiscovery`, `plugin/types.rs:25 PluginConfig`, `plugin/types.rs:44 PluginDescriptor`.

## ✅ Criterio de aceptación

1. `cargo check -p code-graph` pasa sin errores.
2. `cargo check --workspace` pasa (code-graph + xavier-core + xavier + codegraph-types).
3. No introducir `unwrap()`/`panic!` nuevos.
4. Para módulos inexistentes: preferir **quitar la declaración** y limpiar usos antes que crear stubs enormes.
5. `cargo clippy -p code-graph -- -D warnings` sin warnings nuevos.
6. Tests existentes de code-graph (`cargo test -p code-graph`) siguen pasando o se marcan `#[ignore]` con justificación.

## 🔧 Alcance de archivos

- `code-graph/src/lib.rs` (quitar mod impact/mcp o crear stubs)
- `code-graph/src/indexer/mod.rs` (fix &self + tipos)
- `code-graph/src/indexer/watcher.rs`, `call_resolution.rs`, `mcp.rs` (crear vacíos/stubs si se elige esa vía)
- `code-graph/src/db/mod.rs` (añadir métodos file_metadata)
- `code-graph/src/plugin/manager.rs` (impl LanguageDiscovery + campos)
- `code-graph/src/api/plugin_routes.rs` (fix imports)
- `code-graph/src/plugin/engine.rs` (fix response.results → response.symbols)

## 🧪 Cómo verificar
```bash
cargo check -p code-graph
cargo check --workspace
```

## 📎 Referencias
- Issues relacionados: #466 (watcher), #468 (call_resolution), #507 (stabilize main).
- PR #508 intentó arreglar esto pero no completó los stubs.
