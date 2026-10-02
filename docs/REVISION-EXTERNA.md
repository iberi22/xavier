# Revisión externa — apps/xavier (e2dcca79 → bd1b0f67)

Fecha: 2026-10-01. Método: lectura de código, consultas de solo lectura al sistema
(nombres de fichero en `~/.xavier/secrets`, `/proc/<pid>`, journal) y dos subagentes
de lectura para ingesta y para access/rate-limit/settings. No he recompilado ni
relanzado la suite: los gates ya estaban pasados. No he hecho ninguna petición al
daemon vivo.

He verificado en el código cada hallazgo que dejo aquí. Los que no pude reverificar
llevan **[no reverificado]**, y las medidas que tomó un subagente llevan **[medido por
subagente]**.

Lo que ya sabíais (CPU, el 2.º ciclo del cursor, que `SecretEgressPolicy` no tiene
callers y gitleaks) no se repite como defecto.

---

## 1. Veredicto

**La fase no se puede cerrar.** La bloquean tres cosas:

1. **ab88be65.** `GET/PUT /v1/clavis/keys/{name}` lee, y en ciertos casos sobrescribe,
   **cualquier** secreto del vault de respaldo del nodo: `JWT_PRIVATE_KEY`,
   `DB_MASTER_KEY`, `SWAL_DEPLOYER_KEY` y 292 `node_secret_*`. No se limita a los de
   Clavis (D1).
2. **aeadf4ba.** El gate «no podar sin 30 días de evidencia» no lo llama nadie. El
   pruner TGD corre en producción cada 24 h de uptime, y el buffer de accesos se pierde
   en cada `systemctl restart` (D3, D4).
3. **bd1b0f67.** En un repo público, el pre-commit pierde la guarda de SQLite/WAL/SHM y
   el anti-fuga de estrategia (D9).

## 2. Defectos (por gravedad)

### D1 · ALTA · `/v1/clavis/keys/{name}` alcanza todos los secretos del nodo — ab88be65

- `src/adapters/inbound/http/handlers/clavis.rs:44,58` usa
  `HardwareVault::new("xavier-clavis")`, como si el `service_name` aislara los nombres.
  Solo aísla el keyring del SO.
- El almacén de respaldo (`src/secrets/vault.rs:24-61`) es un único `OnceLock` global:
  - un solo directorio, `~/.xavier/secrets/<key>.enc` (l.38);
  - una sola clave, derivada del master key **sin** el nombre de servicio (l.51-56).
- `get_secret` (`vault.rs:132-147`) cae al respaldo ante cualquier error del keyring,
  `NoEntry` incluido. `store_secret` (`vault.rs:114-129`) escribe en el respaldo cuando
  el keyring falla.
- `validate_key_name` (`clavis.rs:265-288`) acepta `DB_MASTER_KEY`, `JWT_PRIVATE_KEY`,
  `node_secret_xv1-…` y `api_key_<n>`.

**Cómo se demuestra.** En este nodo, `~/.xavier/secrets/` tiene 296 ficheros: entre
ellos `DB_MASTER_KEY.enc`, `JWT_PRIVATE_KEY.enc`, `JWT_PUBLIC_KEY.enc`,
`SWAL_DEPLOYER_KEY.enc` y 292 `node_secret_*.enc` (solo listé los nombres).

`GET /v1/clavis/keys/JWT_PRIVATE_KEY` con el token raíz recorre este camino:

1. el keyring `xavier-clavis` responde `NoEntry`;
2. `try_fallback_get` descifra `JWT_PRIVATE_KEY.enc` con la clave compartida;
3. el handler devuelve **200 con la clave privada de firma en claro**.

Como test: dos `HardwareVault` con `service_name` distinto y el **mismo**
`isolated(dir, key)`, que es lo que hace el backend real. Un `store` en uno se lee
desde el otro.

**Impacto aunque la ruta sea solo de admin:**
- Hasta ab88be65 ningún endpoint HTTP devolvía el valor de un secreto. `/secrets/*`
  presta o ejecuta con lease y auditoría (`src/secrets/exec.rs:1-6`). Clavis devuelve el
  valor sin lease y sin dejar rastro en `/secrets/history`.
- Con un token raíz filtrado ya no basta con rotarlo. Se pueden sacar `JWT_PRIVATE_KEY` y
  `DB_MASTER_KEY`, que sobreviven a esa rotación.
- Un `PUT` con el keyring caído sobrescribe `DB_MASTER_KEY.enc` o un `node_secret_*`.
  Eso deja la BD de auth2 indescifrable o el nodo desemparejado. Los 292 `node_secret`
  en el respaldo indican que en este nodo el keyring falla a menudo.
- El commit dice «Export es la única vía que lo revela». No es así:
  `GET /v1/clavis/keys/api_key_<name>` devuelve la key de `xavier keys` sin TTY ni
  confirmación.
- El probe del `PUT` (`clavis.rs:172-179`) contesta `created:false` para nombres de
  otros servicios, así que sirve de oráculo para saber qué secretos existen.

### D2 · MEDIA · El gate de `/v1/clavis/*` en el router productivo no tiene test — ab88be65

- Las rutas están montadas dos veces: `routes.rs:199-216` y `src/cli/server.rs:1039-1059`.
  El que sirve el daemon es el de `server.rs`.
- Hoy el gate está y, leyendo el código, es correcto:
  - `require_permission(can_manage_secrets)` en `server.rs:1043-1056`;
  - `auth_middleware` aplicado en `server.rs:1632`;
  - `xav_`, lease y sesión reciben `UserRole::User` (`http_setup.rs:192-250`).
- Pero todos los tests de router de `clavis.rs:580-702` usan `create_router()`, y ningún
  test del árbol monta el router de `server.rs` para `/v1/clavis`. Si se quita el
  `.layer(...)` en `server.rs`, quedan **0 tests en rojo** (mutación razonada, no
  ejecutada). Es la regla «wiring real» y el mismo patrón que #2793/#2800.
- c729d9b1 ya lo resolvió para `/secrets/*` con `secrets_routes()`, `route_layer` y
  `deny(dead_code)`. ab88be65 es posterior y no lo sigue.
- Falta `/v1/clavis/` en `ROOT_ONLY_PREFIXES` (`http_setup.rs:40`). Un `xav_` con scope
  `all` pasa la capa de scopes y solo lo para la de rol: una capa donde c729d9b1 exige
  tres.

### D3 · ALTA · El gate de prune de aeadf4ba no está conectado al pruner que borra

- `prune_precondition`, `prune_block_reason` y `record_is_observable`
  (`src/memory/access.rs:429,460,505`) solo se llaman desde `access_tests.rs`.
- `TgdUtilityPruner::prune_memories` (`src/memory/tgd.rs:194`) borra con
  `store.delete` (`:250`) sin consultarlos.
- Ese pruner corre desde `garbage_collect` (`src/memory/manager/gc.rs:112-120`).
  `MemoryDaemon` lo programa cada 24 h (`src/scheduler/daemon.rs:100-115`) y se lanza en
  `src/workspace/state.rs:408`, dentro del arranque HTTP.
- Journal desde el 20-09: una pasada de GC (2026-09-28 21:45, `Low-utility pruned: 0`).
  Es decir, el pruner está vivo y ejecutándose.
- El pruner lee el `RECORDER` en memoria, no la tabla `memory_access`, y la hidratación
  solo ocurre en la primera lectura de usuario de cada workspace (`access.rs:350`)
  **[no reverificado]**. Tras un reinicio, un workspace sin lecturas cuenta como «nunca
  observado».

### D4 · ALTA · El buffer de accesos solo se vacía al leer, y se pierde en cada reinicio — aeadf4ba

- `maybe_flush` (`access.rs:370`) tiene un único llamador, el propio camino de registro
  (`access.rs:345`). No hay temporizador ni flush al apagar.
- `server.rs` solo captura `ctrl_c` (~`:2171`) y `start_signal_handler`
  (`src/server/http/mod.rs:60`) no tiene llamadores **[no reverificado]**. Así que el
  SIGTERM de `systemctl restart` pierde lo que haya en el buffer, y lo leído en los 30 s
  posteriores a un flush espera hasta la siguiente lectura.
- Junto con D3, la evidencia que el pruner necesita se pierde en cada despliegue.

### D5 · MEDIA · Un flush fallido cuenta dos veces — aeadf4ba

- `flush_pending` (`access.rs:389-405`) escribe un workspace tras otro. Si falla el N-ésimo
  (l.398-401), restaura `flatten(&drained)` **entero**, incluidos los workspaces ya
  escritos. El siguiente flush los vuelve a sumar a `access_count`.
- Al no llamar a `mark_flushed` en el fallo, cada lectura posterior relanza el flush sin
  backoff.

### D6 · MEDIA · Se cuentan accesos a memorias que el agente no ha visto — aeadf4ba

- En `src/server/v1_api.rs:1217-1222` se registran todos los `search_result.documents`
  **antes** de filtrar por `is_primary_memory` y por clearance (`:1228-1238`). Una memoria
  que el solicitante no puede leer suma utilidad y se libra de la poda.
- MCP `mem_search` (`src/server/mcp/tools_memory.rs:429-439`) registra
  `page*limit+1` candidatos, así que la página N vuelve a contar las páginas 1..N-1
  **[no reverificado]**.
- No registran nada: `v1_memories_get` (`v1_api.rs:1872`), los endpoints de contexto,
  `/memory/search` legacy, MCP `memory_context` y la búsqueda de fragments
  **[no reverificado]**.

### D7 · MEDIA · Ingesta: cursores que pierden datos o se envenenan — 061ecf52, c911eec0

- **Hermes** (`src/memory/hermes_importer.rs:172`): `remember(&path, fingerprint)` se
  ejecuta **antes** del import. Si el import falla, o falla un `store.put(...)?` a mitad
  (l.356), el fichero queda marcado como visto. Los `request_dump_*.json` no cambian
  nunca, así que lo que falte no se ingiere hasta el siguiente reinicio. Antigravity sí lo
  hace bien: recuerda solo tras un import correcto.
- **OpenCode, fecha futura** (`src/memory/opencode_importer.rs:330,363-365`):
  `observed_max` no se limita a `now_ms`. Una sola fila con `time_updated` en el futuro
  (deriva de reloj, unidades erróneas) sube el watermark, y el `floor` (watermark − 24 h)
  queda por encima de todas las sesiones normales. La ingesta de OpenCode se para sin
  avisar hasta reiniciar.
- **OpenCode, error de lectura** (`opencode_importer.rs:327`): `read_turns(&conn, &cand.id)?`
  aborta el `sync` antes de guardar `seen` y el watermark (l.360-366). Si una fila falla
  siempre, cada ciclo lo relee todo hasta ella con watermark 0, que es el escaneo completo
  que el cursor debía evitar.

### D8 · MEDIA · `cb05d059` deja Codex fuera del patrón

- `src/cli/server.rs:787-796` sigue construyendo `CodexImporter` **dentro** del bucle y
  hace `scan_sessions()` más `import_session` de todo en cada ciclo. Son 135 `.jsonl` y
  ~158 MB leídos cada 600 s **[medido por subagente]**.
- Para retirar `XAVIER_INGESTION_INTERVAL_SECS=0` hace falta una medición limpia, y este
  escaneo la contamina.

### D9 · MEDIA · bd1b0f67 quita guardas del pre-commit

El diff de `.husky/pre-commit` borra:
- `scripts/check-strategy-leak.sh --staged` (anti-fuga de estrategia/economía/PII de la
  auditoría 2026-09-10). El script sigue en el repo y el repo es público.
- El bloqueo de `*.db|*.sqlite|*.sqlite3|*-wal|*-shm` staged, que es regla de CLAUDE.md.
  Ahora solo se bloquean ficheros de más de 2 MB, así que un `-wal`/`-shm` pequeño pasa.
  El `BAD_TRACKED` del pre-push tampoco cubre `-wal`/`-shm`.
- `scripts/check-secrets.sh` obligatorio. Sin gitleaks, el escaneo se salta salvo con
  `SWAL_REQUIRE_GITLEAKS=1`.

En `.husky/pre-push`:
- El rango sale de `@{u}` y no de las refs que git pasa por stdin. En una rama sin
  upstream escanea **todo** el historial (604 fixtures), así que el push siempre falla y
  se acaba usando `--no-verify`. En ese caso el filtro de mensajes recorre `HEAD..HEAD` y
  no revisa nada.
- La detección de `--force` lee `$*`, pero git le pasa `<remote> <url>`: es código muerto.

### D10 · MEDIA · Estado de runtime: arranque frágil y escritura no atómica — 6cc0ccf8

- **Arranque** (`src/settings/serialization.rs:110-128`): un fichero de estado con JSON
  inválido se ignora, pero si es JSON válido y `merge_runtime_state` (`:155`) falla,
  `load()` devuelve `Err` y `main.rs:64` (`XavierSettings::load()?`) aborta. Escenario
  realista: un binario nuevo que cambia el tipo de un campo, contra un estado viejo.
  Resultado: bucle de `Restart=on-failure`. **[no reverificado: qué cambios de esquema
  hacen fallar el merge]**
- **Escritura** (`serialization.rs:365-373`): `truncate(true)` sobre el propio fichero,
  sin tmp+rename y sin lock. Un crash o dos escritores a la vez (HORMER y endpoints de
  settings) dejan un fichero truncado. Ese fichero se ignora en silencio, y el siguiente
  `save` guarda los defaults encima del estado aprendido.

### D11 · MEDIA · El estado de runtime guarda secretos y los fija — 6cc0ccf8

- El estado vivo `~/.local/state/xavier/xavier.runtime.json` tiene `security.token_secret`,
  `embedding.api_key` y `pgheart.token` **con valor** (lo comprobé con `jq`, sin
  imprimirlos). En la config versionada están a `null`.
- Por los `is_none()` de `current()`, el estado manda: rotar esos valores en `.env` no
  tiene efecto **[mecanismo no reverificado; presencia sí]**.
- `config/.xavier-state/` no está en `.gitignore` (`git check-ignore` devuelve 1). Con
  `XAVIER_CONFIG_PATH` fijado (`serialization.rs:322,330`), un estado con secretos acaba
  dentro del repo. Es la trampa n.º 1 otra vez.

### D12 · MEDIA · `xavier keys` emite keys que nada valida — ab88be65

- `src/secrets/keygen.rs:176-184` genera `xavier_live_<64hex>` y `:240` calcula
  `expires_at`. En `src/` no hay ningún consumidor de `xavier_live_`, `api_key_` ni
  `value_entry` fuera de `keygen.rs` y `cli/commands/secrets.rs`.
- `auth_middleware` (`http_setup.rs:102-272`) no acepta esas keys, así que el daemon
  responde 401. La caducidad y `revoke_key` (`keygen.rs:279-286`) solo tocan metadatos, y
  `export_key` (`:290-293`) devuelve una key caducada sin avisar.

### D13 · BAJA

- **TTL de keys.**
  - Por CLI no se puede emitir una key sin caducidad: `--ttl-secs 0` significa «default»
    (`enums.rs:1063`, `cli/commands/secrets.rs:85-90`), mientras que en `keygen.rs:71,240`
    `0` es «no caduca».
  - Con `u64::MAX`, `ttl_secs as i64` (`keygen.rs:240`) da −1 y la key nace caducada;
    valores ≥ ~9,2e15 hacen panic en chrono.
- **Masker** (`clavis.rs:142,169` y `src/clavis/mod.rs:47-52`): cada GET/PUT registra el
  valor en un `HashSet` global que no se purga nunca. Un valor común de 4 o más
  caracteres reescribe todas las líneas que pasen por `mask_log_message`.
- **Intervalo de ingesta** (`server.rs:2190-2195`): `XAVIER_INGESTION_INTERVAL_SECS="0 "`
  o `"0s"` no parsean, caen a 600 y **reactivan** el bucle frenado.
- **Directorios de importers** (`server.rs:711-716`): `resolve_*_dir()` se evalúa una
  sola vez. Si el directorio no existe al arrancar, queda fijado.
- **Métrica de Antigravity** (`antigravity_importer.rs:283-284`): `store_reads` solo
  cuenta cuando `get` devuelve `Some`, así que se queda corto justo en la métrica que se
  usa como prueba en producción.
- **ADR-037** (l.46) cita `t5_ventana_fija_admite_doble_en_la_frontera`, que no existe.
  El test real es `t5_la_ventana_rueda_desde_el_primer_request_no_desde_el_reloj`
  (`sliding_window.rs:746`).

## 3. Tests que mienten

| Test | Por qué sigue verde sin la mitigación |
|---|---|
| `clavis.rs::route_get_key_requires_claims`, `route_get_key_forbidden_for_non_admin_role`, `route_proxy_requires_claims_and_returns_501_for_admin` | Montan `routes.rs`, no el router que sirve el daemon. Quitar el gate de `server.rs` → 0 rojos. |
| `clavis.rs::get_key_response_body_is_not_written_to_logs` | No captura logs: comprueba que el masker conoce el valor. `tracing::*` no pasa por el masker, así que un `tracing::info!(%value)` lo dejaría verde. |
| `clavis.rs::stored_value_never_appears_in_clavis_logs_or_on_disk` (parte 1) | Enmascara un string sintético, no la salida del handler. |
| Todos los de clavis/keygen con `isolated(...)` o `MemKeyVault` | Un directorio por instancia: no pueden ver D1 por construcción. |
| `http_setup.rs::scope_policy_tests` (c729d9b1) | Prueban `required_scope`/`token_satisfies`/`lease_may_access` como funciones. Cambiar la llamada de `http_setup.rs:229` por `RequiredScope::Read`, o quitar el `if !lease_may_access` (l.176) → 0 rojos. Del gate de rol sí hay prueba real: `no_non_admin_principal_reaches_any_secrets_route` sobre `secrets_routes()`. |
| `access_tests.rs::prune_is_blocked_until_the_observation_window_elapses`, `prune_precondition_reports_the_binding_constraint`, `only_observable_records_are_prune_eligible` | Prueban funciones que el pruner no llama (D3). |
| `access_tests.rs::a_restored_flush_keeps_the_recorded_accesses` | Ejercita drain/restore, no la rama de error de `flush_pending`. Borrar `access.rs:400` → verde. |
| `access_tests.rs::flush_is_due_once_the_batch_threshold_is_reached` | Prueba el predicado, no que producción dispare un flush (D4). |
| `access_tests.rs::many_reads_batch_into_one_flush` | Pasaría con 200 sentencias sueltas. Además, la rama de 30 s de `should_flush` puede hacerlo flaky **[no reverificado]**. |
| Wiring MCP de accesos | No hay test: quitar `tools_memory.rs:439` → 0 rojos **[no reverificado]**. |
| OpenCode `first_sync_imports_every_session`, `repeated_sync_cycles_never_duplicate`; Hermes `*_second_pass_no_reencode`, `test_hermes_changed_content_reembeds`; Antigravity `test_antigravity_second_pass_no_reencode`, `sync_grown_transcript_is_reread` | Prueban el dedup del store, o usan `import_all`. Con el cursor retirado dan lo mismo. Los que sí fallan sin cursor son 10 de OpenCode, 2 de Hermes y 3 de Antigravity. |
| `server.rs::importers_are_built_before_the_cycle_loop` (~2293) | Usa la primera coincidencia: un constructor fuera más otro dentro del bucle pasa **[no reverificado]**. |
| `server.rs::cursor_stats_are_logged_at_info` (~2348) | `info_logs >= 3` cuenta también el banner, así que bajar un log a debug pasa **[no reverificado]**. |
| `settings::test_default_host_is_loopback_not_wildcard`, `test_default_settings` | Fijan `default_host()`, pero el bind real sale de `resolve_http_bind_host()` (`cli/config.rs:139`): `XAVIER_HOST` > config versionada + estado. Volver la config a `0.0.0.0` → 0 rojos. |
| `settings::test_runtime_state_path_is_outside_the_config_file` | Fija `XAVIER_CONFIG_PATH`, así que prueba `state_path_beside` y no la rama XDG de producción. Con una ruta absoluta de tempdir, `!starts_with("config")` es trivialmente cierto **[no reverificado]**. |
| `settings::test_corrupt_runtime_state_falls_back_to_defaults` | Solo cubre errores de sintaxis, no la rama de esquema que aborta el arranque (D10). |
| `settings::test_load_config_json` (`settings/mod.rs:308`) | Sigue leyendo `config/xavier.config.json` versionado (trampa n.º 2). |

Faltan tests para: el reintento de un import fallido, `time_updated` en el futuro, un
error en `read_turns`, el reinicio con cursor perdido y el flush al apagar.

## 4. Riesgos de producción

1. **D1 probablemente ya está desplegado.** El commit dice «verificado E2E contra el
   servidor vivo». Mientras siga, quien tenga el token raíz puede sacar `JWT_PRIVATE_KEY`,
   `DB_MASTER_KEY` y los secretos de nodo.
2. **El pruner TGD borra sin gate.** Hace la primera pasada tras 24 h de uptime
   continuado, sobre la BD de 1,45 GB. Hasta ahora no ha borrado nada (0 el 28-09), pero
   con D3 y D4 la decisión se toma sin la evidencia que el commit dice exigir. El backup
   de las 03:17 es la única red.
3. **Pista de la CPU (no viene de estos commits, no verificado como causa).**
   - El daemon (pid 2559355) tiene **224 477 fds abiertos**, con un límite de 524 288.
     Casi todos son `/proc/N/task/N/stat` **[desglose medido por subagente]**.
   - `sysinfo` guarda en caché ficheros `stat` abiertos hasta la mitad del límite
     (262 144), y `run_checks` hace `sysinfo::System::new_all()` en cada pasada
     (`src/observability/health.rs:391`). Eso refresca todos los procesos e hilos del
     sistema.
   - Encaja con «la CPU sube también con la ingesta frenada». Merece un perfil antes de
     seguir buscando en la ingesta.
4. **Rotar no surte efecto.** Hasta que se borre el estado de runtime, cambiar
   `token_secret` o `embedding.api_key` en `.env` no hace nada (D11).
5. **Un despliegue puede dejar el daemon en bucle de reinicio** si cambia el esquema de
   settings (D10).
6. **Para levantar el freno de ingesta:**
   - el primer ciclo tras cada reinicio es un barrido completo. En
     `~/.hermes/sessions` hay 1654 ficheros y 440 MB, y 1155 claves `(session, idx)`
     tienen contenido distinto entre dumps, así que se reescriben y reembeben en un
     orden no determinista **[medido por subagente]**;
   - Codex sigue con escaneo completo (D8);
   - un `"0 "` mal escrito reactiva el bucle (D13).
7. **Si se activa el webhook de Telegram, escucha en `0.0.0.0`.** El bind está fijo a
   `[0,0,0,0]` en `src/telegram/mod.rs:460` cuando hay `webhook_url`. No es de 56d87a31,
   pero queda fuera del «loopback por defecto».
8. **Antes de conectar `SecretEgressPolicy` a `/v1/clavis/proxy`:**
   - `is_forbidden_egress_ip` (`src/security/egress.rs:29-53`) no bloquea `100.64.0.0/10`
     (CGNAT/Tailscale; solo `100.100.100.200`), `0.0.0.0/8` salvo `0.0.0.0`, NAT64
     `64:ff9b::/96` ni 6to4 `2002::/16`;
   - `pinned_client` no llama a `.no_proxy()`: con `HTTPS_PROXY` en el entorno no hay pin
     de DNS y el secreto sale hacia el proxy.

## 5. Veredicto por commit

| Commit | Estado | Motivo |
|---|---|---|
| 56d87a31 loopback por defecto | PARCIAL | 8006 y 8100 escuchan en 127.0.0.1 (`ss`, **[medido por subagente]**). Ningún test cubre `resolve_http_bind_host`, y el estado de runtime o `XAVIER_HOST` pueden volver a abrirlo sin que nada se ponga rojo. |
| c729d9b1 gate de `/secrets/*` y scopes | PARCIAL | Gate de rol cerrado por construcción y con test sobre `secrets_routes()`. Las capas de scope y de lease no tienen test en el punto de llamada. `/v1/clavis/` no entró en `ROOT_ONLY_PREFIXES`. |
| 9a43b250 (+8c1240f8) egress | ABIERTO | Sin callers (ya conocido). Además: rangos sin cubrir y proxy de entorno (riesgo 8). |
| 061ecf52 Hermes incremental | PARCIAL | El skip funciona. `remember` antes del import pierde ficheros ante un fallo (D7). |
| c911eec0 cursor OpenCode | PARCIAL | `>=` con ventana de 24 h está bien con empates. El watermark se envenena con una fecha futura y un `?` aborta el pase y tira el cursor (D7). |
| aeadf4ba registro de accesos | ABIERTO | Gate no conectado al pruner (D3), buffer perdido en SIGTERM (D4), doble conteo (D5), conteo previo al filtrado (D6). |
| 9fcbb2bb rate limit atómico | PARCIAL | Atómico y correcto, pero en un limiter sin call-sites (el ADR lo reconoce). El ADR cita un test que no existe. |
| ab88be65 vault Clavis + `xavier keys` | ABIERTO | D1 (todos los secretos del nodo por HTTP), D2 (gate productivo sin test), D12 (keys inertes), D13. |
| 6d3b0845 cursor Antigravity | PARCIAL | Código correcto: recuerda solo tras un import correcto. Falta medir el 2.º ciclo en producción (ya conocido) y `store_reads` se queda corto. |
| cb05d059 importers fuera del bucle | PARCIAL | Los tres importers están bien cableados con `sync()`. Codex sigue dentro del bucle con escaneo completo (D8). |
| 6cc0ccf8 estado fuera de la config | PARCIAL | Ya nadie escribe `config/xavier.config.json`. Abortos de arranque y escritura no atómica (D10), secretos fijados y `.xavier-state` sin ignorar (D11). |
| bd1b0f67 hooks compartidos | ABIERTO | Quita la guarda de SQLite/WAL/SHM, el anti-fuga de estrategia y `check-secrets` obligatorio. El pre-push falla siempre en ramas sin upstream (D9). |
