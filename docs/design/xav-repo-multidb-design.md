# Diseño — BDs de memoria por proyecto, on-demand e interconectadas (XAV-REPO, último 15 %)

- Fecha: 2026-10-04 · Rama: `skills/sync-drift-2026-10-04` · Crate `xavier` 0.2.17
- Encargo: `.gitcore/sdd/xav-repo-multidb-brief.md`
- Estado: **propuesta**. No hay código de producción en este cambio, y ninguna memoria existente se ha leído, movido ni migrado.
- Regla de lectura: cada afirmación sobre el código cita `ruta:línea` leída en esta sesión. Lo que no se pudo verificar se marca **[NO VERIFICADO]** junto con el motivo.

---

## 0. Resumen ejecutivo

1. **Se extiende `ConnectionManager`. No se crea un gestor paralelo.** El store de memoria **ya** corre sobre él: `VecSqliteMemoryStore::new_with_provider` registra un pool `vec_store_<sha12(path)>` (`src/memory/sqlite_vec_store/mod.rs:61-64,108-109`) a través de `GlobalConnectionProvider` (`src/memory/connection_provider.rs:59-73`). Un store por repo es otra instancia del mismo tipo con otra ruta. Un segundo gestor competiría por los mismos fd y la misma RAM sin ver al primero, que es justo el defecto que hoy tiene el caché aparte de code-graph (§1.3).
2. Antes de ponerle encima muchas BDs, el `ConnectionManager` necesita **cuatro arreglos**: (a) la expulsión hace `pool.get()` bloqueante con el lock global de escritura tomado; (b) el pool del store global puede ser expulsado; (c) no hay barrido de inactivos; (d) el perfil de cada pool (10 conexiones × 8 MiB de caché + 256 MiB de mmap) es demasiado caro para N stores.
3. **Interconexión: fan-out en el servidor + merge por rango (RRF).** Se descartan ATTACH, la vista unificada y la tabla de referencias como mecanismo de búsqueda. La búsqueda vectorial es un *scan* exhaustivo sin índice ANN y BM25 no es comparable entre índices FTS5 distintos, así que ATTACH no aporta nada medible y añade límites duros.
4. **Consistencia:** cada memoria tiene un solo store dueño. No hay transacciones distribuidas. Las referencias entre stores son URIs que se resuelven en lectura y pueden quedar colgantes de forma explícita.
5. **Cifrado:** el `.sqlite3` vivo **no** se cifra entero (no hay SQLCipher en las dependencias). Se mantiene el cifrado por registro de `at_rest.rs`, pero con una **clave de registro por repo** en lugar de la del nodo, y el paquete `.enc` (`repo_package_crypto`) se usa solo para commitear.
6. **Migración:** no se mueve ni una fila. El store global queda como capa *legacy* de solo lectura en búsquedas federadas y sigue siendo el destino por defecto de las escrituras sin repo. **Repartir las 22.217 filas por repo no se puede hacer sin migrar y sin heurística**, y este diseño no lo intenta.
7. **Sincronización entre máquinas: hoy no es posible.** La KEK del paquete deriva de la master key del nodo, y esta está ligada a la máquina. Hace falta un ADR de compartición de claves, y eso queda fuera de alcance.

---

## 1. Estado actual verificado

### 1.1 Correcciones al brief (verificadas en el código)

| # | El brief dice | El código dice |
|---|---|---|
| C1 | `ConnectionManager` «hoy se usa solo para el codegraph (`get_code_graph_db`)». | **Al revés.** `get_code_graph_db` **no usa los pools**: delega en `code_graph::db::CodeGraphDB::new` (`src/codebase/connection_manager.rs:247-252`), que tiene su propio caché (`code-graph/src/db/mod.rs:20-47`). Los pools r2d2 sirven `memory`, `metrics`, `security`, `default`, `auth`, `vec_store_*` y `conv_*` (`src/cli/server.rs:246-249`, `src/security/initializer.rs:38`, `src/memory/sqlite_vec_store/mod.rs:109`, `src/codebase/conversations_db.rs:132-133`). |
| C2 | `xavier_context_save` escribe en `VecSqliteMemoryStore`. | Crea un checkpoint en `conversations_db` (`src/server/mcp/tools_context.rs:132-149`), es decir, en el pool `conv_<workspace>` → `~/.xavier/conversations/<id>.db` (`connection_manager.rs:168-176`). |
| C3 | `xavier repo pack\|unpack` y «la BD de memoria por repo». | `pack` solo empaqueta `code_graph.db` (`src/cli/commands/repo_pack.rs:50,259-266`). La memoria no se empaqueta. |
| C4 | `memory_store_path_for()` es la «selección». | Ningún caller de runtime la usa: `rg` solo la encuentra en su propio módulo. Además, para el repo del daemon resuelve a `<data_dir>/memory.sqlite3` (`src/memory/repo_memory_store_path.rs:135-137`), un fichero **nuevo**, no `vec-store.sqlite3`. |
| C5 | Dos productos de un monorepo quedan aislados. | **Solo el `project_id`.** `derive_repo_identity` pone `root` = raíz git (`src/codebase/repo_identity.rs:157-159,175-177`) y `memory_store_path_for_identity` usa `identity.root` (`repo_memory_store_path.rs:163-165`). Resultado: `apps/duque-mvp` y `apps/tripro-web` irían **al mismo** `<gitroot>/.xavier/memory.sqlite3`. El test del monorepo solo comprueba ids distintos (`src/codebase/repo_config.rs:650-696`), y afirma además que comparten raíz (`:677-682`). El comentario de `repo_memory_store_path.rs:157-162` («each product carries its own root») es falso hoy. |
| C6 | 811k filas «de grafo» en `timeline_events`. | `timeline_events` es la cadena de auditoría (`src/storage/migrations.rs:276-290`, escrita en `src/memory/sqlite_vec_store/audit.rs:72-84`). El grafo son `entities`/`relations`/`memory_entities` (`migrations.rs:219-263`). **[NO VERIFICADO]** el número de filas: no se ha abierto la BD de producción. |
| C7 | 1,45 GB (CLAUDE.md) / 1,57 GB (brief). | `ls -la data/` da `vec-store.sqlite3` = **1.573.040.128 B** y `-wal` = 3.534.992 B. El dato de CLAUDE.md está desfasado. |
| C8 | (no lo menciona) | Existe **un tercer mecanismo multi-BD**, `MultiDbManager` (`src/storage/multi_db.rs:17-26`), montado en `POST/GET /v1/workspaces/db` y `DELETE /v1/workspaces/db/{id}` (`src/cli/server.rs:1059-1063`). Su registro vive solo en memoria y no tiene expulsión (§1.3). |
| C9 | `MemoryMax=1G`. | `docs/research/2026-08-24-agent-scanner-dedup-incident.md:33` dice que se subió a `MemoryMax=4G` mediante el drop-in `memory.conf`. **[NO VERIFICADO]**: esta sesión no tiene permiso para leer la unit. El diseño dimensiona para **1 GiB**, el peor caso. |

### 1.2 `ConnectionManager` (`src/codebase/connection_manager.rs`, 635 líneas)

- Estado: `pools`, `known_paths` y `active` (`:25-33`); `ProjectPool { pool: Arc<Pool>, activated_at: Instant }` (`:35-38`); `MAX_POOLS = 16` (`:17`); `idle_timeout_secs = 300` (`:140`).
- `connect()` mapea ids literales a rutas, y todo lo demás cae en `<root>/.xavier/codebase.db` (`:151-196`, catch-all `:182-186`). `connect_with_path()` registra `known_paths` (`:200-202`), abre la conexión de init y fija WAL (`:213-217`), y construye el pool con **`max_size(10)`** (`:225-229`). **Después** llama a `evict_if_needed()` (`:231`), así que durante un instante hay 17 pools.
- Resurrección: `get_or_reconnect_pool` resucita contra `known_paths` y rechaza ids del catch-all que nunca se registraron (#2736; `:295-339`, `is_explicitly_mapped` `:113-117`).
- **Defecto D1 (bloqueo global):** `evict_if_needed` toma `self.pools.write()` (`:373`) y, **con el lock tomado**, hace `entry.pool.get()` (bloqueante; en r2d2 0.8 espera hasta `connection_timeout`, 30 s por defecto) y `wal_checkpoint(TRUNCATE)` (`:385-386`, `:401-402`). Mientras tanto, **cualquier** `with_conn` de cualquier store del proceso espera, porque `get_or_reconnect_pool` también toma `pools.write()` (`:300`).
- **Defecto D2 (lock exclusivo en el camino caliente):** cada `with_conn` → `get_or_reconnect_pool` toma el lock **de escritura** solo para actualizar `activated_at` (`:300-304`). Con fan-out a N stores, esto serializa todas las consultas en un único mutex.
- **Defecto D3 (sin barrido):** la expulsión por inactividad solo corre dentro de `connect_with_path` al crear un pool **nuevo** (`:231`). Un store inactivo nunca se cierra si no entra otro.
- **Defecto D4 (sin clases):** el LRU elige entre **todos** los pools (`:393-397`), incluido el store global `vec_store_<sha>` y `memory`/`security`. Abrir 16 repos expulsaría el store de producción.
- `disconnect` (`:266-268`) y `shutdown` (`:260-263`) no hacen checkpoint de los pools. El comentario de `shutdown` habla de «flush all SQLite WAL checkpoints», pero solo vacía el caché de code-graph. En la práctica SQLite hace checkpoint al cerrar la última conexión de un fichero WAL, de modo que el `TRUNCATE` explícito de la expulsión es casi redundante y su `get()` bloqueante es peor que inútil.
- Coste por pool (`src/storage/pragma.rs:43-53`): `cache_size=-8000` (≈7,8 MiB por conexión, `:49`), `mmap_size=268435456` (256 MiB, `:50`), `wal_autocheckpoint=1000` (`:46`) y `journal_size_limit=10485760` (`:47`). r2d2 0.8 abre `min_idle` (por defecto = `max_size`) conexiones al construir el pool (`Cargo.toml:366` → `r2d2 = "0.8"`). Según la documentación de r2d2 0.8, eso son **10 conexiones abiertas en cuanto se registra un store**. **[NO VERIFICADO EN RUNTIME]**: la tarea 3 lo mide.

**Pools fijos del daemon** (estimación estática; la tarea 3 los cuenta de verdad): `memory`, `metrics` (compartido por `TimeMetricsStore`, `SecurityThreatStore` y `QmdAuditLogger`: `src/time/mod.rs:32-33`, `src/security/threat_store.rs:28-29`, `src/secrets/audit.rs:148-149`), `security` (`src/security/audit.rs:34`, `src/security/tokens.rs:42`), `default` (`server.rs:249`, `src/security/user_store.rs:41`), `auth` (`initializer.rs:38`), `vec_store_<global>`, el `vec_store_<…>` de `service_log` (`src/observability/service_log.rs:231-232`) y `conv_<ws>` por workspace. Son **≈8 + W de 16**, así que quedan ~7 huecos antes de que el LRU empiece a expulsar infraestructura.

### 1.3 Los otros dos mecanismos multi-BD

- **code-graph** (`code-graph/src/db/mod.rs:20-47`): una `Connection` por ruta, con tope de 64. Al superarlo **vacía todo excepto la recién insertada** (`cache.retain(|p,_| *p == norm_path)`, `:44-46`). No es LRU. El comentario documenta un agotamiento real de fd («ulimit 1024», `:35-37`).
- **`MultiDbManager`** (`src/storage/multi_db.rs`): BDs con nombre en `<data_dir>/db/<id>.sqlite` (`:47-55`). Cachea `VecStore` sin límite (`:25`, `:166-188`); el registro `databases` vive solo en memoria y nada lo recarga al arrancar. `get_store` no tiene ningún caller fuera de sus tests (`rg`). **Riesgo:** sus tests resuelven la ruta con `XavierSettings::current()` (`:48-54`) y escriben en el `data/db/` **real**. Ese directorio existe, está vacío y tiene mtime de hoy (`ls -la data/db`). Viola la trampa 3 de CLAUDE.md.
- Higiene relacionada: `.xavier/tests/` acumula **27.618 ficheros** de BDs `conv_test_*`/`test_*` filtradas por los tests (`ls | wc -l`). La causa es el mapeo `test_*` → `<root>/.xavier/tests/<id>.db` con `root="."` (`connection_manager.rs:163-181,324`).

### 1.4 Store de memoria (`src/memory/sqlite_vec_store/`)

- Construcción: `new_with_provider` registra el pool (`mod.rs:108-109`), corre **11 migraciones** y la migración de embeddings (`schema_impl.rs:63-90`), comprueba el modelo de embeddings y puede lanzar un reindex en segundo plano (`schema_impl.rs:123-157`). Abrir un store **no es barato**, así que hay que cachear el objeto store: el struct no retiene el pool (`mod.rs:46-55`) y la expulsión de pools sigue funcionando debajo.
- Esquema relevante: `memory_fts USING fts5(id UNINDEXED, path, content, code_tokens)` (`migrations.rs:206-211`); `memory_embeddings` y `memory_embeddings_768`, que son **tablas normales con BLOB, no `vec0`** (`migrations.rs:199-203`, `:594-598`), con PK `id` y **sin índice en `workspace_id`**.
- Búsqueda (`backend_impl.rs:49-266`): `COUNT(*)` del workspace para un `rrf_k` dinámico (`:72-82`; `utils.rs:118-125`). La pata vectorial es `vec_distance_cosine` sobre **todas** las filas del workspace, `ORDER BY distance LIMIT` (`:92-106`): fuerza bruta. La pata FTS es `bm25(memory_fts)` (`:145-155`). La pata KG es `entities.name LIKE '%term%'` (`:192-196`): scan. Cada fila se descifra en Rust (`:124-127`).
- **Cifrado y FTS:** las filas con `encrypted_dek` **solo indexan el path** en FTS (`store_impl.rs:1377-1390`). ADR-038 mide que **las 22.410 filas son `XRK1`** (`docs/adr/ADR-038-key-recovery-record-key.md:45`). Hoy, por tanto, BM25 **no busca contenido** en producción, solo rutas.
- Escritura: `put` hace `put_store` + `put_index` + `put_link` dentro de una `with_conn` **sin transacción explícita** (`store_impl.rs:107-114`), y el evento de timeline en otra `with_conn` (`:117-119`). `put_link` es un no-op (`:1416-1419`). El enlazado de símbolos bajo demanda abre **siempre el code graph del daemon** (`code_graph_db_path_for(".")`, `store_impl.rs:1433-1436`).
- La clave de registro es **global del nodo**: `resolve_record_key()` (`at_rest.rs:93`) lee `XAVIER_RECORD_KEY` o `<data_dir>/node/record.key` (`at_rest.rs:11-14,40-45`).

### 1.5 Caminos de escritura y lectura del daemon

- `/memory/add` (`src/cli/server.rs:1637-1642` → `src/cli/handlers/memory.rs:444`): `workspace_id = state.workspace_id` (`:585`), una constante del daemon, y escribe en `state.memory` (`:607`). Después **replica en un `QmdMemory` en RAM** en segundo plano (`:610-642`).
- `/memory/search` (`handlers/memory.rs:205`) **no lee SQLite**: `MemoryQueryEngine::search(&state.qmd_memory, …)` (`:281-291`). Aplica el techo de *clearance* del solicitante (`:251-268`). `QmdMemory` en modo lazy carga **todos** los documentos del workspace, embeddings incluidos, en la primera búsqueda (`src/memory/qmd/mod.rs:217-229`, `src/memory/qmd/reader.rs:270-286`; `src/workspace/state.rs:193-222`).
- **Consecuencia de seguridad:** `row_matches_filters` de la ruta SQL (`mod.rs:231-263`) filtra workspace/project/scope/session pero **no clearance**. Una búsqueda federada por SQL tiene que aplicar el techo de clearance ella misma, o lo saltaría (F2.2).
- El MCP corre en el **mismo proceso** que HTTP (`server.rs:1925-1957`).
- La resolución de repo por petición (codegraph, `src/cli/handlers/code.rs:108-137`) hace `canonicalize` y **lanza `git rev-parse` en cada petición** (`repo_identity.rs:120-134`). Además usa `derive_project_id` en lugar de `derive_repo_identity`, así que ignora el `project_id` declarado en `config.toml`. No se cachea nada.

### 1.6 Paquete cifrado (`src/codebase/repo_package_crypto.rs`)

- KEK = HKDF-SHA256(master, `xavier-repo-package-kek-v1:<project_id>`) (`:80,101-115`). La master se resuelve con `MasterKeyManager::load_or_init` y su copia de respaldo en `~/.xavier/master.key` está envuelta con material del host y la máquina (`repo_pack.rs:116-124`; ADR-038 `:18-30`). **Un `.enc` no se abre en otra máquina** salvo que comparta la master.
- `encrypt_file` hace `std::fs::read` del fichero entero (`:267`): el pico de RAM es aproximadamente 2 × el tamaño. `pack` cifra el `.db` **tal como está en disco** (`repo_pack.rs:280`), sin checkpoint ni snapshot, así que puede perder las páginas que aún estén en el `-wal`. `unpack` borra `-wal`/`-shm` después del rename (`repo_pack.rs:397-404`), lo que es **destructivo si hay un proceso con el fichero abierto**.

### 1.7 Observabilidad y health

- `src/health/mod.rs` **no se toca**. Corrigió dos señales falsas (commits `04f4232a` y `d68b44e5`): (1) contaba embeddings en la columna vieja en vez de en `memory_embeddings_768`; (2) informaba de una ruta inexistente (0 MB, «healthy») en vez del store real. Las lecciones aplicables son: **medir donde están los datos de verdad** e **informar del fichero que existe y está abierto**, nunca de uno deducido de la configuración.
- Hoy no existe ninguna API que diga cuántos pools hay abiertos, cuáles se han expulsado o cuánta memoria usa SQLite.

---

## 2. Decisión de arquitectura

### 2.1 Gestor de conexiones: extender `ConnectionManager`

| Opción | A favor | En contra | Veredicto |
|---|---|---|---|
| **A. Extender `ConnectionManager`** | El store de memoria ya vive ahí (`mod.rs:108-109`). `known_paths` ya resuelve el caso «id hasheado + resurrección» (#2736). Un único presupuesto de fd/RAM para todo el proceso. `InMemoryProvider` (`connection_provider.rs:79-92`) ya da aislamiento en tests. | Hay que corregir D1-D4 y añadir perfiles. Afecta a todos los pools. | **Elegida** |
| B. Gestor paralelo solo para memoria por repo | Aísla el cambio. | Dos LRU compitiendo por los mismos fd sin verse, que es el problema de code-graph (§1.3). `VecSqliteMemoryStore` habría que reescribirlo para no usar `ConnectionProvider`, o duplicar el provider. | Descartada |
| C. Reutilizar `MultiDbManager` | Ya tiene endpoint. | Caché sin límite (`multi_db.rs:25`), registro volátil, rutas en `data_dir` y no en `<repo>/.xavier`, y ningún caller real de `get_store`. | Descartada (converger más tarde: §6, «NO se hará») |

Encima del `ConnectionManager` va una capa fina de dominio, `RepoStoreRegistry` (memoria). **No es un gestor de pools**: resuelve `raíz → ruta`, aplica la allowlist y el opt-in, cachea objetos `VecSqliteMemoryStore` (ya migrados) y deduplica aperturas concurrentes.

### 2.2 Apertura y cierre on-demand: números

**Perfiles de pool** (nuevo `PoolProfile`, tarea 4):

| Perfil | `max_size` | `min_idle` | `cache_size` | `mmap_size` | Clase |
|---|---|---|---|---|---|
| `Default` (hoy, sin cambio) | 10 | = max (10) | −8000 KiB | 256 MiB | `Pinned` para ids de infraestructura y store global |
| `RepoStore` | 4 | 0 (perezoso) | −2000 KiB | 0 | `Evictable` |

**Presupuesto de peor caso** (estimación; la tarea 5 la mide con `/proc/self/fd` y `sqlite3_memory_used()`):
- RAM por store de repo: 4 conexiones × ~2 MiB de caché + ~0,5 MiB de overhead por conexión ≈ **10 MiB**. El perfil `Default` es 10 × 7,8 MiB ≈ **78 MiB** de caché heap, más hasta 256 MiB de páginas mmap compartidas por fichero (caché de páginas que el cgroup contabiliza aunque sea reclamable). 16 pools `Default` llenos son ≈ 1,25 GiB solo de caché: **ya hoy superarían 1 GiB** si todos los ficheros fueran grandes. Solo no ocurre porque la mayoría son pequeños (`security.db` 16 KiB).
- fd: SQLite en Unix abre por conexión un fd de BD y uno de WAL, más un `-shm` por fichero y proceso. `RepoStore`: 4 × 2 + 1 = **9 fd**. `Default`: 10 × 2 + 1 = **21 fd**.
- **Tope recomendado:** `XAVIER_MAX_REPO_STORES = 6` pools `Evictable` calientes (≈60 MiB, ≈54 fd), con un límite aparte para los `Pinned`. Con `MemoryMax=1G` cabe con holgura. Con 4G se puede subir a 16 sin cambiar el diseño.
- WAL por store: acotado por `wal_autocheckpoint=1000` páginas (~4 MiB) y `journal_size_limit` de 10 MiB (`pragma.rs:46-47`).

**Política:**
- **Abrir:** solo en la primera petición que nombra un repo con opt-in (§2.5). La resolución está cacheada, así que no se abre `.xavier/` en cada visita.
- **Cerrar:** (a) LRU entre `Evictable` al superar `XAVIER_MAX_REPO_STORES`; (b) barrido cada 60 s que cierra los `Evictable` inactivos más de `XAVIER_REPO_STORE_IDLE_SECS` (por defecto 300); (c) `shutdown`. Al cerrar, se intenta el checkpoint con `try_get()` (no bloqueante) **fuera** del lock del mapa. Si no hay conexión libre, se omite: al soltarse la última conexión, SQLite hace checkpoint y borra el WAL él solo.
- **Nunca** se expulsa un `Pinned` (store global, `memory`, `metrics`, `security`, `default`, `auth`, `conv_*`).
- **Reabrir tras expulsión:** `known_paths` resucita sin repetir migraciones, porque el objeto store sigue cacheado en el registry. Si el fichero se sustituyó mientras tanto (`unpack`), el registry invalida la entrada y la reabre completa (tarea 12).

### 2.3 Interconexión: fan-out + merge en el servidor

Coste real **con este esquema** para cada alternativa:

| Opción | Coste y límites concretos | Veredicto |
|---|---|---|
| **ATTACH** | El `ATTACH` es estado **por conexión**: con pools de 10 conexiones habría que adjuntar en cada *acquire* o mantener conexiones dedicadas. El límite es `SQLITE_MAX_ATTACHED` = 10 en rusqlite `bundled` (`Cargo.toml:317`). La pata vectorial es un scan sin índice (`backend_impl.rs:92-106`): `UNION ALL` sobre BDs adjuntas cuesta lo mismo que N consultas, sin ningún ahorro. `bm25()` usa las estadísticas de **cada** tabla FTS5 por separado, así que las puntuaciones no son comparables. El descifrado por fila y los filtros ocurren en Rust (`:124-128`) y no se expresan en una sola SQL. Las escrituras que crucen BDs en WAL son atómicas por fichero pero **no** entre ficheros (documentación de SQLite). **Un `.enc` no se puede adjuntar.** | Descartada |
| **Vista unificada** (`TEMP VIEW … UNION ALL`) | Tiene todos los límites de ATTACH y además pierde el `MATCH` de FTS5: `MATCH` requiere la columna-tabla de **una** tabla FTS5 concreta, y una vista sobre varias no lo propaga. | Descartada |
| **Tabla de referencias** | Sirve para **enlazar** registros entre proyectos, no para **buscar**. | Complemento (§2.4), no mecanismo de búsqueda |
| **Fan-out + merge en el servidor** | Reutiliza `hybrid_search_with_embedding` sin cambios (`mod.rs:326-343`). Calcula el embedding **una vez**. Coste ≈ Σ filas de los stores consultados, frente a hoy, que escanea las 22k filas de `memory_embeddings_768` porque no hay índice por `workspace_id`. Al partir por repo, cada scan es más pequeño. Permite timeout por store, resultados parciales y concurrencia acotada. | **Elegida** |

**Semántica del merge (tarea 11):**
- Resultado global = RRF con `k = 60` constante (`DEFAULT_RRF_K`, `config.rs:12`) sobre:
  - (a) **una sola lista vectorial global**, ordenada por similitud coseno cruda. Es comparable entre stores **solo si** todos usan el mismo modelo (`embedding_model_meta.active`). Si un store tiene otro modelo, se omite su pata vectorial y se marca `skipped_model_mismatch`.
  - (b) las listas léxica y KG **de cada store por rango**: BM25 y el `rrf_k` dinámico no son comparables entre stores.
- Hoy la similitud cruda se pierde: se pasa como `bm25` y se ignora para `Vector` (`backend_impl.rs:122,130-137`). Hace falta un campo nuevo `vector_similarity: Option<f32>` en `HybridSearchResult` (`src/memory/store.rs:118-128`; hay 3 constructores).
- Cada resultado lleva `store` (`project_id` o `legacy`). Se deduplica por `(store, memory_id)`.
- Respuesta: `per_store: [{store, ms, hits, status: ok|timeout|error|skipped_model_mismatch|not_opted_in}]`. Un store que falla **no** tumba la búsqueda, pero queda registrado en la respuesta.
- Concurrencia: `Semaphore(4)` por consulta y timeout de 2 s por store. Cada `with_conn` ya corre en `spawn_blocking` (`connection_manager.rs:349-354`), así que no hay Rayon dentro de Tokio.
- **Clearance:** se post-filtra con el techo del solicitante, igual que `search_handler` (`handlers/memory.rs:253-268`), porque la ruta SQL no lo aplica (§1.5).
- El camino actual (`QmdMemory`) **no cambia** cuando la petición no nombra repos. La federación es opt-in por petición (`repos: [...]`). La pata `legacy`, cuando se incluye, lee el store global por SQL en solo lectura. **Su ranking será distinto del de `QmdMemory`**, y eso se documenta en la respuesta.

### 2.4 Consistencia entre proyectos

- **Un dueño por memoria.** El store en que se escribió es el único que tiene la fila.
- **Referencias entre stores:** URI `xavier://<project_id>/<memory_id>`, guardada en el store que referencia. No hay FK entre ficheros (SQLite no las admite entre BDs adjuntas). La resolución es perezosa: si el destino falta o su store no está permitido, el resultado es `unresolved`, y nunca un error ni un dato de otro repo.
- **Sin 2PC.** Una operación que toque dos stores son dos transacciones, en este orden: dueño primero, referencia después. Si el daemon muere entre ambas, falta la referencia, pero **nunca queda una referencia colgante hacia una fila inexistente creada a medias**. La referencia es idempotente.
- **Snapshots:** una búsqueda federada **no** es una instantánea consistente entre stores. Cada store se lee en su propia transacción de lectura WAL. Para búsqueda es aceptable y se declara.
- **Si el daemon muere:** WAL + `synchronous=NORMAL` (`pragma.rs:48`). Ante una caída del **proceso** no se pierde nada confirmado. Ante una caída de **corriente** se pueden perder las últimas transacciones, sin corrupción. **Ya hoy** se pierde además: (1) una escritura a medio índice, porque `put` no es transaccional (`store_impl.rs:107-114`) y puede quedar `memory_records` sin fila FTS o sin embedding (la tarea 15 lo corrige); (2) la réplica `QmdMemory`, porque es un `tokio::spawn` (`handlers/memory.rs:625`). Los stores de repo no usan `QmdMemory`.
- La tabla de referencias (`memory_links`) se crea **solo en stores de repo**, con su propia lista de migraciones. Añadirla a la lista compartida (`schema_impl.rs:70-82`) la aplicaría al store global en el siguiente arranque, y eso contradice «no tocar lo existente».

### 2.5 Rutas y ciclo de vida

- **Quién abre:** solo el **daemon** (HTTP y MCP están en el mismo proceso, `server.rs:1925-1957`). El CLI **no** abre stores de repo mientras el daemon corre, salvo `pack`, que usa `VACUUM INTO` (lectura consistente bajo WAL, el mismo método que ya usan los backups según CLAUDE.md), y `unpack --offline`.
- **Raíz del store:** `nearest_config_ancestor(dir)` (`repo_identity.rs:187-196`) y, si no hay, `find_repo_root(dir)` (`:95-117`). Esto corrige C5: el producto de un monorepo tiene su propio `.xavier/memory.sqlite3` junto a su `config.toml`, coherente con `repo pack` (`repo_pack.rs:95-100`), que ya escribe junto al producto.
- **Opt-in obligatorio:** la raíz debe tener `.xavier/config.toml` con `memory_store = "repo"` (nueva clave en `SETTABLE_KEYS`, `repo_config.rs:382`), y además debe estar bajo `XAVIER_REPO_STORE_ROOTS`. Si esa variable está vacía, la feature queda desactivada. Sin estas dos condiciones, un cliente HTTP podría hacer que el daemon cree `/<cualquier>/.xavier/memory.sqlite3`.
- **Caché de resolución:** LRU de 256 entradas `raw_root → (store_root, project_id, path)`, con TTL de 60 s para recoger cambios de `config.toml`. **No** se llama a `git_head` en el camino caliente: el commit no hace falta para abrir un store de memoria.
- **Rutas canónicas** antes de `project_id_for_path` (`mod.rs:61-64`): el id es el sha del *string* de la ruta, así que dos grafías del mismo fichero darían dos pools sobre el mismo fichero.
- **MCP:** puede leer y escribir en stores **ya existentes** y con opt-in. **Nunca** puede crearlos, desempaquetarlos ni adoptar filas legacy, porque la ingesta es un canal de inyección de prompts (CLAUDE.md) y esas operaciones son HTTP-admin.

### 2.6 Cifrado

| Opción | Efecto | Veredicto |
|---|---|---|
| Cifrar el `.sqlite3` vivo entero | rusqlite usa `bundled` y no `bundled-sqlcipher` (`Cargo.toml:317`): SQLite no puede operar sobre un fichero cifrado. Haría falta descifrar a disco o a RAM en cada apertura, lo que deja texto plano en disco o cuesta el tamaño del fichero en RAM. Impide FTS, ATTACH y `VACUUM INTO`. | Descartada |
| SQLCipher | Cambio de dependencia y de formato para **todos** los stores, incluido el global, lo que contradice «no tocar». | Fuera de alcance |
| **Cifrado por registro (`at_rest.rs`) en vivo + `.enc` para commitear** | El código ya existe. `content` y `metadata` cifrados. Se filtran: `path`, embeddings en claro (invertibles de forma aproximada), `entities.name` (= path, `graph.rs:85-99`) y `timeline_events.summary` (= operación + path, `audit.rs:82`). BM25 solo sobre path (`store_impl.rs:1380-1384`), igual que hoy en producción. | **Elegida** |

Matiz obligatorio: la clave de registro **no puede ser la del nodo** para un store pensado para commitearse. Si lo fuera, el `.enc` se abriría con la KEK del repo pero las filas seguirían cifradas con `node/record.key`, una clave distinta. Decisión: **clave de registro por repo** = HKDF(master, `xavier-repo-record-key-v1:<project_id>`), que tiene el mismo dominio de portabilidad que la KEK del paquete. El store global sigue con la clave del nodo, sin cambios.

**Límite honesto:** la portabilidad entre máquinas sigue siendo **imposible** sin compartir la master. Resolverlo (concesión explícita por máquina, frase de recuperación por repo, etc.) requiere un ADR propio y **no** forma parte de estas tareas.

### 2.7 Migración sin mover filas

- **Fase actual (estas tareas):** el store global sigue `Pinned`, se abre al arrancar como hoy y es el destino de toda escritura que **no** nombre un repo con opt-in. Ninguna migración nueva entra en la lista compartida. Los stores de repo nacen vacíos.
- **Lectura:** con `repos` en la petición y `include_legacy=true` (por defecto), la pata `legacy` consulta el store global **en solo lectura** con el `workspace_id` actual. Leer un store nuevo no cambia nada del viejo.
- **Repartir las 22.217 filas por repo no es posible sin migrar.** Su `workspace_id` es por agente o `default` (desglose del brief), no por repo, y `clearance_level` es NULL al 100 %. Cualquier reparto necesitaría una heurística (p. ej. `metadata.namespace.project`), y esta sesión no ha medido su cobertura porque no se ha abierto la BD.
- **Camino futuro (no se ejecuta):** `xavier repo adopt --dry-run` (tarea 16) solo **cuenta** candidatas por `namespace.project`, en solo lectura. Una adopción real sería **copiar** (no mover) recifrando con la clave del repo, con tabla de procedencia, y los originales intactos. Eso requiere un ADR y una petición explícita de BELA.

### 2.8 Observabilidad

- `ConnectionManager::snapshot()` → `[{id, path, profile, class, connections, idle_connections (r2d2 Pool::state), idle_for_ms}]`, más contadores `opened_total`, `evicted_total{lru,idle,manual}` y `resurrected_total`, y un anillo de las últimas 32 expulsiones `{id, reason, at, checkpointed: bool}`.
- `RepoStoreRegistry::stats()` → stores abiertos, aciertos y fallos de resolución, y rechazos (`not_opted_in` / `outside_allowlist`).
- Memoria: `rusqlite::ffi::sqlite3_memory_used()` (heap real de SQLite) y `VmRSS` de `/proc/self/status`.
- Consulta lenta: spans de `tracing` por pata (`vector`, `fts`, `kg`, `decrypt`) en `perform_hybrid_search`. Si una pata supera 500 ms, se registra `{store, leg, ms, rows_scanned}` en un anillo de 64 entradas.
- Endpoint `GET /v1/admin/stores`: **solo HTTP-admin, nunca MCP**.
- `health_check`: añadir **solo** una sección derivada del `snapshot()` (lo que está abierto de verdad). Un store cerrado se informa como `measured: false`, **nunca** como 0 o *healthy*. Requiere tocar `src/health/mod.rs`, así que es una tarea aparte con autorización explícita (tarea 14).

### 2.9 Pruebas

Todas son deterministas: `tempdir`, `ConnectionManager::new()` aislado (o `InMemoryProvider`) y **nunca** el singleton `global()` ni `data/`. «Disco lleno» se reproduce con `PRAGMA max_page_count`, que da `SQLITE_FULL` de forma determinista sin trucos de sistema de ficheros. El reloj es inyectable (`evict_idle_at(now)`). Los nombres concretos van en cada tarea. Cada tarea incluye su **mutation test** (CLAUDE.md, «Cómo se demuestra»).

---

## 3. Tareas pendientes

Convenciones:
- Comandos de verificación: solo los tres de CLAUDE.md.
- «Proceso real» = instancia aislada con `XAVIER_DATA_DIR=$(mktemp -d)`, puerto 18006/18100 y binario de `cargo build -p xavier --release --bin xavier` **sin** `ci-safe`. **Nunca** el daemon de producción ni `data/vec-store.sqlite3`.
- 1 PR = 1 tarea, salvo donde se indique.
- Orden: las dependencias van entre corchetes.

### T1 — Higiene de tests que escriben fuera de tempdir
- **Ficheros:** `src/storage/multi_db.rs` (constructor `MultiDbManager::with_root(PathBuf)`; los tests `:190-296` pasan un tempdir), `src/codebase/connection_manager.rs` (el test `:615-634` usa `connect_with_path` a un tempdir en vez de `"."`).
- **Tests:** `multi_db::tests::lifecycle_never_touches_settings_data_dir`. Mantener verdes los 3 tests existentes de `multi_db`.
- **Aceptación:** tras `cargo test … multi_db connection_manager`, el `mtime` de `data/db/` no cambia y `ls .xavier/tests | wc -l` no crece (hoy 27.618). Mutation: restaurar `resolve_db_path` en el test → `data/db` cambia de mtime.
- No incluye: limpiar los 27.618 ficheros existentes. Es una acción manual que debe confirmar BELA.

### T2 — ADR-039 y requisito
- **Ficheros:** `docs/adr/ADR-039-per-repo-memory-stores.md` (contexto → decisión → consecuencias de §2), `docs/SRS/REQUIREMENTS.md` (nuevo REQ: aislamiento físico por repo con opt-in), `docs/features/repo-memory-instance.md` (enlace).
- **Aceptación:** el ADR cita C1-C9, deja como rechazadas ATTACH, la vista unificada, el cifrado completo, el gestor paralelo y la reutilización de `MultiDbManager`, y declara que la sincronización entre máquinas queda fuera.

### T3 — `ConnectionManager::snapshot()` y contadores (sin cambio de comportamiento) [T1]
- **Ficheros:** `src/codebase/connection_manager.rs`.
- **Tests:** `connection_manager::tests::snapshot_lists_open_pools_with_real_paths`, `snapshot_counts_evictions_by_reason`, `snapshot_reports_pool_state_from_r2d2`.
- **Aceptación:** el snapshot devuelve ruta, conexiones y `idle_for_ms` de cada pool. En el **proceso real** se registra en el PR el número de pools y de fd tras el arranque (cierra la incógnita de §1.2 y verifica `min_idle`). Mutation: devolver un `Vec` vacío → los 3 tests en rojo.

### T4 — `PoolProfile` y clases `Pinned`/`Evictable` [T3]
- **Ficheros:** `src/codebase/connection_manager.rs` (`connect_with_path_profile`; `connect_with_path` = perfil `Default`, idéntico al de hoy), `src/storage/pragma.rs` (función aditiva `apply_profile_overrides`; `apply_connection_pragmas` no cambia), `src/memory/connection_provider.rs` (método de trait con implementación por defecto), `src/cli/server.rs` (marcar como `Pinned` los ids de infraestructura y el store global en `:246-255`).
- **Tests:** `repo_profile_applies_cache_mmap_and_pool_size` (`PRAGMA cache_size = -2000`, `mmap_size = 0`, `max_size = 4`, `state().connections == 0` tras construir), `default_profile_is_byte_identical_to_today`. `test_pooled_connections_carry_both_pragma_layers` (`:434-460`) sigue verde.
- **Aceptación:** sin regresión en el baseline (2936/0/1). Mutation: ignorar el perfil → 1 test en rojo como mínimo.

### T5 — Expulsión no bloqueante, barrido de inactivos y lock de lectura en el camino caliente [T4]
- **Ficheros:** `src/codebase/connection_manager.rs` (sustituir `pool.get()` de `:385,401` por `try_get()` **fuera** del lock; LRU solo entre `Evictable`; `activated_at` como `AtomicU64` para que `get_or_reconnect_pool` use `pools.read()`; `evict_idle_at(now)` público), `src/cli/server.rs` (tarea de barrido cada 60 s).
- **Tests:** `eviction_never_evicts_pinned_pools`, `eviction_does_not_block_when_victim_pool_is_exhausted` (retiene las 4 conexiones de la víctima y abre un pool nuevo: debe terminar en < 1 s), `idle_sweep_closes_only_idle_evictable_with_injected_now`, `evicted_pool_leaves_no_wal_after_last_conn_drops`, `hot_path_parallel_queries_do_not_serialize` (64 tareas × 4 pools, sin deadlock en 5 s), `repo_store_churn_200_opens_keeps_fd_bounded` (Linux; cuenta `/proc/self/fd`).
- **Aceptación:** **mutations** documentadas: volver a `pool.get()` → `…does_not_block…` en rojo por timeout; quitar el filtro de clase → `…never_evicts_pinned…` en rojo. En el proceso real se abren 20 repos de prueba y el store global no aparece nunca en el anillo de expulsiones.

### T6 — Raíz del store por producto (corrige C5) [T2]
- **Ficheros:** `src/codebase/repo_identity.rs` (`RepoIdentity.package_root: Option<String>` con `#[serde(default)]` para no romper el protocolo CLI↔servidor; se rellena con `nearest_config_ancestor`), `src/memory/repo_memory_store_path.rs` (`memory_store_path_for_identity` usa `package_root` y, si no hay, `root`; se corrige el comentario de `:157-162`).
- **Tests:** `monorepo_products_resolve_to_distinct_memory_files`, `identity_without_package_root_falls_back_to_root` (JSON viejo sin el campo).
- **Aceptación:** mutation: volver a `identity.root` → el test del monorepo en rojo.

### T7 — `RepoStoreRegistry` con opt-in, allowlist y apertura única [T5, T6]
- **Ficheros:** `src/memory/repo_store_registry.rs` (nuevo), `src/memory/mod.rs`, `src/memory/sqlite_vec_store/mod.rs` (constructor aditivo `new_with_profile`), `src/codebase/repo_config.rs` (clave `memory_store = "global"|"repo"`, por defecto `global`, en `SETTABLE_KEYS` `:382`).
- **Comportamiento:** canonicaliza la ruta; single-flight por clave (`tokio::sync::OnceCell` por entrada); caché de resolución LRU 256 con TTL 60 s; rechazos tipados (`NotOptedIn`, `OutsideAllowlist`, `StoreCorrupt`).
- **Tests:** `two_repos_two_files_no_cross_read`, `store_opened_once_migrations_run_once` (contador de aperturas), `concurrent_open_same_repo_yields_one_pool` (32 tareas), `spelling_variants_share_one_pool`, `root_without_opt_in_is_rejected_and_creates_nothing`, `root_outside_allowlist_is_rejected`, `corrupt_store_file_is_reported_and_other_stores_keep_working` (basura en `memory.sqlite3`), `disk_full_write_fails_cleanly_without_partial_row` (`PRAGMA max_page_count`).
- **Aceptación:** ningún test usa `ConnectionManager::global()`. Mutation: quitar la comprobación de opt-in → test en rojo y fichero creado.

### T8 — Clave de registro por repo [T7] (debe aterrizar **antes** de T10)
- **Ficheros:** `src/memory/sqlite_vec_store/at_rest.rs` (resolución con override por store), `src/memory/sqlite_vec_store/mod.rs` (campo `record_key_override`), `src/memory/sqlite_vec_store/store_impl.rs` (`:1298`, la escritura usa la clave del store), `src/memory/sqlite_vec_store/backend_impl.rs` (`:67`, la lectura usa la clave del store), `src/codebase/repo_package_crypto.rs` (derivación `xavier-repo-record-key-v1:` junto a la de la KEK).
- **Tests:** `repo_store_rows_unreadable_with_node_key`, `global_store_still_uses_node_key` (regresión), `repo_record_key_is_domain_separated_from_package_kek`.
- **Aceptación:** mutation: usar la clave del nodo en el store de repo → el primer test en rojo. **Ningún** cambio en filas existentes.

### T9 — Enlazado de símbolos contra el code graph del propio repo [T7]
- **Ficheros:** `src/memory/sqlite_vec_store/store_impl.rs` (`:1433-1436`: usar `<store_root>/.xavier/code_graph.db` cuando el store es de repo), `src/memory/sqlite_vec_store/mod.rs` (`repo_root: Option<PathBuf>`).
- **Tests:** `repo_store_links_against_its_own_code_graph`, `global_store_link_path_unchanged`.
- **Aceptación:** mutation: volver a `"."` → test en rojo.

### T10 — Escritura enrutada por repo (HTTP y MCP) [T8, T9]
- **Ficheros:** `src/cli/handlers/memory.rs` (`AddPayload.repo: Option<RepoRef{root, project_id}>`; si es válido, se escribe **solo** en el store de repo y sin réplica en `QmdMemory`), `src/cli/state.rs` (campo `repo_stores`), `src/cli/server.rs` (construcción del registry; se extrae, si hace falta, el builder del router `large_body_routes` `:1637` para testear el router productivo), y los constructores de `CliState` en tests (`src/secrets/exec_http_tests.rs:164`, `src/cli/handlers/navigation.rs:378`, `src/cli/handlers/code.rs:2892`, `src/cli/handlers/ollama_models.rs:309`). MCP: argumento opcional `repo` en `memory_save`, sin creación de stores.
- **Tests:** sobre el **router productivo**: `add_with_repo_writes_only_repo_store`, `add_with_repo_leaves_global_store_file_byte_identical` (sha256 de un store global de *fixture* en tempdir, antes y después), `add_without_repo_behaves_exactly_as_today`, `mcp_cannot_create_repo_store`.
- **Aceptación:** mutation: enrutar al global → 2 tests en rojo. En el proceso real, una escritura con `repo` aparece en `<repo>/.xavier/memory.sqlite3` y el global no cambia de tamaño.

### T11 — Búsqueda federada [T10]
- **Ficheros:** `src/memory/federated_search.rs` (nuevo), `src/memory/store.rs` (`HybridSearchResult.vector_similarity`, `#[serde(default)]`), `src/memory/sqlite_vec_store/search.rs` y `backend_impl.rs` (propagar la similitud cruda, `:122-137`), `src/cli/handlers/memory.rs` (`SearchPayload.repos`, `include_legacy`; ruta federada **solo** si `repos` no está vacío; techo de clearance aplicado), y la tool MCP `memory_search` con el argumento `repos` (solo lectura).
- **Tests:** `merge_is_rank_based_and_deterministic`, `vector_merge_uses_raw_similarity_across_stores`, `model_mismatch_skips_vector_leg_and_flags_store`, `slow_store_times_out_and_result_is_partial_with_status`, `clearance_ceiling_applies_to_federated_results`, `legacy_leg_is_read_only` (sha256 del fixture sin cambios), `no_repos_uses_qmd_path_unchanged`.
- **Aceptación:** mutation: quitar el post-filtro de clearance → `clearance_ceiling…` en rojo. En el proceso real se mide la latencia p50/p95 de 1, 3 y 6 stores y se anota en el PR.

### T12 — Paquete de memoria con snapshot consistente [T7]
- **Ficheros:** `src/cli/commands/repo_pack.rs` (`--memory`: `VACUUM INTO <tmp>` → `encrypt_file` → `memory.sqlite3.enc`; **aplicar el mismo snapshot a `code_graph.db`**, que hoy lee el fichero en crudo en `:280`; `unpack` toma antes un lock exclusivo del destino y se niega si está abierto; `--offline` es explícito), `src/cli/commands/enums.rs` (flags), `src/memory/repo_store_registry.rs` (`invalidate(key)` para que el daemon reabra tras un `unpack` desde HTTP-admin).
- **Tests:** `pack_memory_snapshot_includes_uncheckpointed_wal_rows`, `unpack_refuses_when_target_is_open`, `unpack_never_deletes_wal_of_open_db`, `config_init_writes_gitignore_excluding_live_dbs` (este último toca además `src/codebase/repo_config.rs` `init_config` `:420`), y los 28 tests existentes de `repo_pack` en verde.
- **Aceptación:** mutation: sustituir `VACUUM INTO` por la lectura en crudo → el primer test en rojo. **Riesgo anotado:** `encrypt_file` hace `fs::read` completo (`repo_package_crypto.rs:267`), con un pico de RAM de unas 2× el tamaño. Corre en el proceso CLI, no en el cgroup del daemon. Pasar a streaming exige un formato v2 y queda fuera.

### T13 — `GET /v1/admin/stores` y log de consultas lentas [T3, T7]
- **Ficheros:** `src/cli/handlers/stores_admin.rs` (nuevo), `src/cli/handlers/mod.rs`, `src/cli/server.rs` (ruta con permiso admin; **no** registrada en MCP), `src/memory/sqlite_vec_store/backend_impl.rs` (spans por pata y anillo de lentas).
- **Tests:** `admin_stores_requires_admin_scope` (router productivo), `admin_stores_reports_only_actually_open_pools`, `slow_query_ring_records_leg_and_store`.
- **Aceptación:** en el proceso real, abrir 3 repos y expulsar 1 queda reflejado en el endpoint con su motivo. `sqlite3_memory_used()` no es 0.

### T14 — Sección `stores` en `health_check` [T13] — **requiere autorización explícita de BELA para tocar `src/health/mod.rs`**
- **Ficheros:** `src/health/mod.rs` (solo aditivo).
- **Tests:** `health_stores_section_reflects_snapshot`, `closed_store_reported_as_not_measured_not_healthy`.
- **Aceptación:** la sección sale **solo** de `snapshot()`; no deduce rutas de la configuración (la lección de `d68b44e5`). Mutation: devolver `healthy` para un store cerrado → test en rojo.

### T15 — `put` transaccional (hueco previo, afecta también al global) [independiente]
- **Ficheros:** `src/memory/sqlite_vec_store/store_impl.rs` (`:107-114` dentro de un `unchecked_transaction`, igual que ya se hace en `:192`, `:752` y `:1487`).
- **Tests:** `put_failure_in_index_leaves_no_orphan_record` (forzar un fallo en `put_index` con un fixture sin `memory_fts`).
- **Aceptación:** mutation: quitar la transacción → test en rojo. No mueve ni modifica filas existentes; solo cambia la atomicidad de escrituras nuevas.

### T16 — `xavier repo adopt --dry-run` (solo lectura, informe) [T11]
- **Ficheros:** `src/cli/commands/repo.rs` (subcomando), `src/memory/repo_store_registry.rs` (consulta de conteo).
- **Tests:** `adopt_dry_run_counts_by_namespace_project_on_fixture`, `adopt_dry_run_opens_global_read_only` (`SQLITE_OPEN_READ_ONLY`; sha256 sin cambios).
- **Aceptación:** **no existe** modo de ejecución. El informe da la cobertura real de `namespace.project` y es la evidencia para decidir si una adopción futura es viable.

---

## 4. Riesgos

| Riesgo | Mitigación |
|---|---|
| T4 y T5 tocan el gestor que usan **todos** los stores de producción. | El perfil `Default` es idéntico al actual; los ids de infraestructura son `Pinned`; baseline 2936/0/1; proceso real antes de desplegar. |
| Ranking de la pata `legacy` por SQL distinto del de `QmdMemory`. | La federación solo se activa con `repos`; sin ella, el camino es el de hoy. |
| BM25 casi inútil en filas cifradas (solo path). | Ya pasa hoy (ADR-038). Se declara. Un índice ciego (*blind index*) de términos sería un ADR aparte. |
| Embeddings y paths en claro dentro de un store commiteado en vivo. | Lo que se commitea es el `.enc`, nunca el `.sqlite3`. Hoy **nada** genera un `.gitignore` en `.xavier/` (`rg gitignore` en `repo_config.rs`, `repo.rs` y `repo_pack.rs` no devuelve nada). T12 debe hacer que `repo config init` escriba `.xavier/.gitignore` con `*.db`, `*.sqlite3` y `*-wal`/`*-shm`, y añadir un test. |
| Reindex por cambio de modelo al abrir cada store (`schema_impl.rs:123-157`) con `REINDEX_RUNNING` global (`schema_impl.rs:21-24`). | El registry abre una sola vez por proceso. T11 marca `skipped_model_mismatch` mientras tanto. |
| `MemoryMax` real desconocido (1G frente a 4G). | Se dimensiona para 1 GiB; T3 mide. |
| Otros agentes trabajando en el repo. | Cada tarea toca su propia isla de ficheros; T10 y T11 comparten `handlers/memory.rs` y deben ir en serie. |

## 5. Dependencias y orden

`T1 → T2 → T3 → T4 → T5 → T6 → T7 → T8 → T9 → T10 → T11 → T12 → T13 → (T14 con autorización)`. T15 es independiente; T16 va después de T11.

## 6. Lo que explícitamente NO se hará

- Mover, copiar, recifrar o borrar ninguna de las 22.217 filas del store global, ni añadir migraciones a su lista compartida.
- ATTACH, vistas unificadas o transacciones distribuidas entre stores.
- Cifrar el `.sqlite3` vivo entero o introducir SQLCipher.
- Sincronizar entre máquinas o compartir la master key (necesita su propio ADR).
- Crear, desempaquetar o adoptar stores desde MCP.
- Tocar `src/health/mod.rs` sin autorización (T14), `~/.xavier/master.key` o `data/vec-store.sqlite3`.
- Reescribir `MultiDbManager` o el caché de code-graph. Converger ambos sobre el registry es trabajo posterior, que se anotará en el ADR.
- Quitar `XAVIER_INGESTION_INTERVAL_SECS=0` ni cambiar los backups.
