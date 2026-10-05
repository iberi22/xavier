# Encargo a Claude Code — Diseño: BDs de memoria por proyecto, on-demand, interconectadas

Repo: `/home/belal/proyectosSWAL/apps/xavier` (crate Rust `xavier` v0.2.17, rama
`skills/sync-drift-2026-10-04`).

Tu tarea es **análisis y diseño**, no implementación. Produce un documento de diseño
con tareas pendientes concrete. NO escribas código de producción, NO hagas commit.

## Qué quiere BELA (literal)

> "hay que crear una lógica que permita a Xavier abrir y cerrar ondemand bds de proyectos
> pudiendo manejar muchas bd sin problemas interconectando entre todos sin problemas"

Y el objetivo global de la feature (XAV-REPO): que cada repo git tenga su propia
instancia de memoria, configurable por el usuario, empaquetada y cifrada en
`<repo>/.xavier/` para poder commitearse y sincronizarse. Y: "para ahora no hacer nada
con las memorias que ya tenemos".

## Lo que YA EXISTE y funciona (verificado, no lo re-diseñes)

**Identidad y rutas por repo**
- `src/codebase/repo_identity.rs` — `RepoIdentity{project_id, root, indexed_commit}`;
  `derive_repo_identity()` ya consulta `<dir>/.xavier/config.toml` y el `project_id`
  declarado gana al derivado del nombre del directorio. `nearest_config_ancestor()`
  es público y compartido con el CLI.
- `src/codebase/repo_config.rs` (875 líneas, 23 tests) — config por repo.
- `src/codebase/codegraph_paths.rs` (72) — `code_graph_db_path_for()`: una
  `code_graph.db` por repo en `<repo>/.xavier/`.
- `src/memory/repo_memory_store_path.rs` (369, 8 tests) — `memory_store_path_for()`
  devuelve `<repo>/.xavier/memory.sqlite3`. **Ojo: hoy esto es solo ruta + selección;
  el daemon sigue abriendo un único store global.** Ver "el gap" más abajo.

**Cifrado y empaquetado**
- `src/codebase/repo_package_crypto.rs` (700, 16 tests) — AES-GCM reusando
  `crypto::encryption::{aes_encrypt,aes_decrypt}` y `at_rest::{wrap_dek,unwrap_dek}`.
  KEK por repo: HKDF-SHA256 con info `xavier-repo-package-kek-v1:<project_id>`.
  DEK aleatorio por operación. Contenedor `XAVPKG01`. `indexed_commit` viaja en el header.
  API: `seal`, `open`, `encrypt_file`, `decrypt_file`, `verify_file`,
  `derive_package_kek_from_master`, `package_path_for`, `header_for`.
- `src/cli/commands/repo_pack.rs` (925, 28 tests) — `xavier repo pack|unpack`.
  `unpack` es atómico (temporal + rename). Verificado con el binario real: el `.enc`
  de un repo no se abre desde otro, y un fallo no deja un `.db` a medias.
- `src/cli/commands/repo.rs` — `xavier repo config init|show|set -C <dir>`.

**EL HALLAZGO MÁS IMPORTANTE PARA TU DISEÑO**

`src/codebase/connection_manager.rs` (635 líneas) ya implementa casi exactamente lo que
BELA pide para "muchas BDs on-demand":

```rust
pub struct ConnectionManager {
    pools: RwLock<HashMap<String, ProjectPool>>,   // pool por project_id
    known_paths: RwLock<HashMap<String, PathBuf>>, // path real por project_id, para resucitar
    active: Arc<tokio::sync::RwLock<Option<String>>>,
    idle_timeout_secs: u64,
}
const MAX_POOLS: usize = 16;
// eviction LRU: mientras pools.len() >= MAX_POOLS, saca el mas viejo por activated_at,
// ejecuta PRAGMA wal_checkpoint(TRUNCATE) y lo elimina.

pub fn new() / global() -> &'static Self
pub fn connect(&self, project_id, project_root)
pub fn connect_with_path(&self, project_id, db_path: PathBuf)
pub fn get_code_graph_db(...)
pub fn unload_code_graph_db(&self, db_path) -> bool
pub fn get_or_reconnect_pool(...)
pub fn disconnect(&self, project_id)
pub fn shutdown(&self)
```

O sea: **ya hay pools por proyecto, LRU con tope, checkpoint WAL al expulsar, y
resurrección contra el path real**. Hoy se usa solo para el codegraph
(`ConnectionManager::global().get_code_graph_db(path)`).

Tu diseño debe responder: **¿se extiende este ConnectionManager a las BDs de memoria, o
hace falta un gestor paralelo?** Argumenta con evidencia del código, no por gusto.

## El gap exacto (el 15% que falta)

El daemon abre **un solo store global** de memoria:
`/home/belal/proyectosSWAL/apps/xavier/data/vec-store.sqlite3` (~1.57 GB), con
22.217 filas multiplexadas por la columna `workspace_id`:
`default` 10.599 · `agent:hermes` 9.501 · `agent:opencode` 1.808 ·
`agent:antigravity` 184 · `agent:codex` 125. `clearance_level` es NULL en el 100%.

`VecSqliteMemoryStore` está en `src/memory/sqlite_vec_store/{mod.rs,store_impl.rs,
schema_impl.rs,memory_access_impl.rs,at_rest.rs,config.rs}`. Escribe por
`/memory/add` (HTTP) y `xavier_context_save` (MCP).

**BELA pidió explícitamente NO tocar ni migrar las memorias existentes.** Tu diseño debe
respetarlo y proposer cómo hacerlo sin mover 22.000+ filas.

## Lo que tu documento de diseño debe responder, con evidencia del código

1. **Apertura/cierre on-demand.** Cómo se decide abrir y cerrar el store de un proyecto.
   Reutiliza el LRU de `ConnectionManager` o propón otro. ¿Cuántos simultáneos? ¿Qué pasa
   con file descriptors, WAL, y `MemoryMax=1G` del systemd unit? Números concretos.

2. **Interconexión entre BDs.** BELA quiere "interconectando entre todos sin problemas".
   Una query de búsqueda puede necesitar datos de varios proyectos (p. ej. un agente que
   busca en SWAL y en el repo del cliente). ¿Qué semántica quieres: `ATTACH` de SQLite,
   una vista unificada, un fan-out con merge en el servidor, o tabla de referencias? Analiza
   el coste real de cada una CON ESTE ESQUEMA (FTS5 + sqlite-vec + tabla de grafo de 811k
   filas en timeline_events). Recomienda una y justifica por qué las otras no.

3. **Consistencia entre proyectos.** Qué pasa si un registro se referencia desde dos BDs.
   ¿Snapshots, WAL mode, transactions distribuidas? ¿Qué se pierde si el daemon muere
   a mitad?

4. **Rutas y ciclo de vida.** Quién abre un store: el daemon HTTP, el CLI, el MCP? Dónde
   se resuelve `repo_root -> path` y cómo se cachea. Evita abrir un `.xavier/` por visita.

5. **Cifrado.** El store de memoria por repo, ¿se cifra en reposo como el paquete
   (`repo_package_crypto`) o hereda el cifrado por registro que ya tiene
   `at_rest.rs`? Ojo: cifrar el `.sqlite` completo impide `ATTACH` y FTS. Analiza el
   trade-off y recomienda.

6. **Migración sin romper lo existente.** Cómo pasar de un store global multiplexado a
   uno-por-proyecto SIN perder ni mover las 22.217 filas, y qué pasa con las BDs viejas
   cuando se lee una nueva. Belle explicitly dijo que no se toque ahora: diseña el
   camino, no lo ejecutes.

7. **Observabilidad.** Cómo saber cuántos stores están abiertos, cuál se evictó, cuánto
   memoria, y por qué una query fue lenta. El `health_check` tiene que seguir siendo
   verdad (hoy ya arreglé dos señales falsas ahí: lee `src/health/mod.rs` para no
   repetirlas).

8. **Pruebas.** Cómo se prueba "muchas BDs sin problemas" de forma determinista:
   abrir N, evictar, reconectar, corrupto, disco lleno, concurrencia. Nombra los tests
   concretos.

## Restricciones

- **NO hagas commit. NO escribas código de producción.** Un `.md` de diseño en
  `docs/design/` está bien; código en `src/` no.
- NO toques `src/health/mod.rs`, ni las memorias existentes, ni `~/.xavier/master.key`.
- Hay OTROS agentes trabajando en este repo ahora mismo. No hagas `git checkout`,
  `git reset`, `git restore` de ficheros completos.
- `grep` es alias de `rg` en esta máquina: `grep -E` falla en silencio y devuelve 0 líneas
  que PARECEN un pass. Usa `rg` o `/run/current-system/sw/bin/grep -E`.

## Entregable

Un único documento markdown en `docs/design/xav-repo-multidb-design.md` con:
- El estado actual verificado (con rutas y líneas reales).
- La decisión de arquitectura, con las alternativas analizadas y el porqué de la elegida.
- **Las tareas pendientes, numeradas y con criterios de aceptación verificables**, cada
  una con los ficheros exactos que tocaría y sus tests. El objetivo es que otro agente
  pueda ejecutarlas sin volver a analizar.
- Los riesgos y lo que explícitamente NO se hará.

Sé concreto y honesto: si algo no se puede hacer bien, dilo. Prefiero un diseño que diga
"esto no es posible sin migrar" a uno que prometa un aislamiento que no existe.
