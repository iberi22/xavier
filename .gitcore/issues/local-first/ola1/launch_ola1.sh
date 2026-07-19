#!/usr/bin/env bash
# Orquestador Ola 1 — iniciativa "Xavier 100% Local" (LLM + Embeddings vía Ollama)
# Lanza 14 issues paralelos para Jules, aislados por capa/archivo.
set -euo pipefail

OUT=.gitcore/issues/local-first/ola1/created_issues.txt
: > "$OUT"

create() {
  local num="$1" title="$2" labels="$3" body_file="$4"
  local full="[LOCAL1-${num}] ${title}"
  local url
  url=$(gh issue create --title "$full" --body-file "$body_file" --label "$labels" 2>&1) || {
    echo "LOCAL1-${num} FAILED: $url" | tee -a "$OUT"; return 1; }
  echo "LOCAL1-${num} -> ${url}" | tee -a "$OUT"
  sleep 0.4
}

DIR=.gitcore/issues/local-first/ola1/bodies
mkdir -p "$DIR"

cat > "$DIR/01.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Para que Xavier sea 100% local (LLM + embeddings vía Ollama), el primer obstáculo es que **la función que decide si un proveedor está listo mente**: `ModelProviderConfig::is_configured()` en `src/agents/provider/config.rs:491` devuelve `true` para el modo `Local` mientras la `base_url` no sea vacía — **incluso si Ollama no está corriendo**. Eso hace que el sistema crea tener un proveedor cuando en realidad no hay forma de generar texto, y los errores llegan tarde y crípticos al usuario.

## 📋 Problema

Hoy (`src/agents/provider/config.rs:491`):
- `Local` → `true` si `base_url` no está vacía. Nunca comprueba que el endpoint responda.
- Resultado: el chat del panel intenta usar el proveedor local, falla con error de conexión, y el usuario ve "No pude contactar el modelo de IA".

## ✅ Criterio de aceptación

1. Crear un método síncrono/async `is_reachable()` en `ModelProviderConfig` (o en el `ModelProviderClient`) que haga un `GET {base_url}/models` (Ollama expone `/v1/models` compatible OpenAI) con un **timeout corto (≤2s)** y cachee el resultado en memoria por ~15s para no sondear en cada mensaje.
2. **No romper `is_configured()` existente** (sync, barato): que siga validando campos. Añadir `is_reachable()` como chequeo adicional de red.
3. Distinguir tres estados claros: `Configured & Reachable`, `Configured & Unreachable` (Ollama caído), `NotConfigured`.
4. En modo `Local`, el `api_key` por defecto `"ollama"` debe aceptarse sin exigir clave real.
5. Tests unitarios: `is_configured()` sigue true para local con URL no vacía; `is_reachable()` devuelve `Err`/`false` ante un puerto cerrado y `Ok`/`true` ante un mock HTTP (usar `mockito`).
6. **No modificar** `ProxyUseCase` ni el handler del panel (eso es otro issue paralelo — sólo tocas `src/agents/provider/config.rs` y opcionalmente `client.rs`).

## 🔧 Alcance de archivos (para no chocar con issues paralelos)

- `src/agents/provider/config.rs` (principal)
- `src/agents/provider/types.rs` (añadir tipo de estado si hace falta)
- Tests nuevos.

## 🧪 Cómo verificar

```bash
cargo test -p xavier provider::config::is_reachable
cargo clippy -p xavier -- -D warnings
```

## 📎 Dependencias
Ninguna. Es fundación para los issues de la Ola 2 que cablean el chat.
EOF

cat > "$DIR/02.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

La meta es Xavier 100% local. Ya existe un escáner de sistema (`src/cli/handlers/system_scan.rs:9 SystemScanResult`, `src/cli/onboarding.rs:74 SystemScanner`) que detecta si Ollama está corriendo y qué modelos hay. Pero esa info **sólo se usa en el setup wizard CLI** (`src/cli/handlers/setup.rs:19`), no en el arranque del servidor HTTP. El servidor arranca a ciegas respecto a qué modelos locales tiene disponibles.

## 📋 Problema

- El `SystemScan` se ejecuta sólo por CLI (`xavier setup`).
- Al arrancar el servidor (`src/cli/server.rs`), no se sondea Ollama. El `ProviderRouter` y el `CliState` se inicializan sin saber qué modelos locales existen.
- El headless endpoint `/v1/system/scan` (`src/cli/handlers/headless_api.rs:30`) devuelve info, pero no se invoca en boot.

## ✅ Criterio de aceptación

1. Al iniciar el servidor HTTP, ejecutar un `SystemScan` **una vez** (en background, no bloqueando el bind del puerto) y guardar el resultado en `CliState` como `Arc<RwLock<Option<SystemScanResult>>>`.
2. Si detecta Ollama corriendo: log info `🦙 Ollama detectado: N modelos (...)`. Si el modelo default (`qwen3-coder`) **no está** instalado, `warn!` claro: `"Modelo X no encontrado en Ollama. Ejecuta: ollama pull X"`.
3. Exponer el resultado del scan vía el endpoint existente `/v1/system/scan` (ya en `headless_api.rs:30`) leyéndolo del `CliState` cacheado en vez de recalcular en cada llamada.
4. Refresco periódico opcional: re-scanear cada 5 min (configurable vía `XAVIER_SCAN_INTERVAL_SECS`) para detectar modelos recién instalados.
5. Tests: mock del endpoint `/v1/models` de Ollama (con `mockito`) verificando que el scan parsea la lista y detecta ausencia del modelo default.

## 🔧 Alcance de archivos (aislado)

- `src/cli/handlers/system_scan.rs` (añadir función de scan async reutilizable)
- `src/cli/state.rs` (campo nuevo en `CliState`)
- `src/cli/server.rs` **sólo** en la sección de inicialización (spawn del task de scan) — coordinar via campo nuevo, no tocar rutas de chat.
- `src/cli/handlers/headless_api.rs:30` (leer del cache).

## 🧪 Cómo verificar
```bash
ollama pull qwen3-coder
cargo run -- serve &
curl -s http://localhost:8006/v1/system/scan | jq '.ollama'
```

## 📎 Dependencias
- Se puede avanzar en paralelo con el issue de `is_reachable()` usando el scan directo.
EOF

cat > "$DIR/03.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Existe un `ProviderRouter` completo con soporte de fallback chains (`src/agents/provider/router.rs:172 on_provider_failure`, `set_fallback_chain:164`, `ActiveProvider::Fallback(Vec)`) — pero la cadena **nunca se popula** en el flujo de chat y, peor, `local` no se considera nunca. La infraestructura está, falta configurarla.

## 📋 Problema

- `ProviderRouter::new()` en `src/cli/server.rs:342` se inicializa con un único `ProviderKind::OpenAI` por defecto.
- Nadie llama a `set_fallback_chain()` con una lista que incluya `ProviderKind::Local`.
- Resultado: aunque el router tuviera lógica de fallback, su cadena sólo contiene un proveedor cloud.

## ✅ Criterio de aceptación

1. Añadir `ProviderRouter::build_default_chain(configured_providers: &[ProviderKind]) -> Vec<ProviderKind>` que devuelva una cadena sensata:
   - Primero los proveedores cloud configurados (orden: por policy configurable `LowestLatency`/`BestQuality`).
   - **Siempre** añadir `ProviderKind::Local` al **final** como último recurso si Ollama está `is_reachable()` (issue LOCAL1-01).
   - Si no hay cloud configurado y local sí → cadena = `[Local]`.
2. Al arrancar el servidor, llamar `set_fallback_chain(build_default_chain(...))` con los proveedores que tengan `is_configured() == true`.
3. Exponer la cadena activa en el endpoint `/provider/status` (`src/cli/handlers/provider.rs`) para inspección.
4. Logs: al construir la cadena, `info!("Provider fallback chain: [OpenRouter → DeepSeek → Local(Ollama)]")`.
5. Tests unitarios sobre `build_default_chain` cubriendo: sólo cloud, sólo local, mixto, ninguno (devuelve vacío → quien llama decide error o degradación).

## 🔧 Alcance de archivos (aislado)

- `src/agents/provider/router.rs` (lógica nueva)
- `src/cli/server.rs:342` (una llamada al construir el router — no tocar rutas)
- `src/cli/handlers/provider.rs` (endpoint de status)

**No tocar:** `src/app/proxy_use_case.rs` (esa integración es Ola 2).

## 🧪 Cómo verificar
```bash
cargo test -p xavier provider::router::build_default_chain
curl -s http://localhost:8006/provider/status | jq '.fallback_chain'
```

## 📎 Dependencias
- Conveniente (no bloqueante) el issue LOCAL1-01 (`is_reachable()`) para decidir si incluir `Local`.
EOF

cat > "$DIR/04.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

**Buenas noticias**: el sistema de embeddings ya soporta Ollama local. En `src/embedding/mod.rs`:
- `DEFAULT_LOCAL_EMBEDDING_ENDPOINT = "http://localhost:11434/v1/embeddings"` (línea 16)
- `DEFAULT_LOCAL_EMBEDDING_MODEL = "embeddinggemma"` (línea 17)
- `ProviderMode::{Local, LocalGllm, Cloud, Auto, Disabled}` (línea 38)
- `EmbedderConfig::auto()` (línea 185) con auto-detección de señales local/cloud
- `FallbackEmbedder` con cadena de backends (línea 174)

El problema: la política `auto()` requiere **señales explícitas** (`XAVIER_EMBEDDING_PROVIDER_MODE` o `XAVIER_EMBEDDER` o `XAVIER_MODEL_PROVIDER=local`). Si no hay ninguna, y no hay claves cloud, cae a `Noop` → **embeddings desactivados** → la memoria semántica no funciona.

## 📋 Problema

`EmbedderConfig::auto()` (`src/embedding/mod.rs:185`) → si no hay señal local ni cloud, devuelve `Noop`. Esto significa que una instalación limpia sin configurar nada **no tiene embeddings** y por tanto no hay búsqueda semántica de memoria.

## ✅ Criterio de aceptación

1. Cambiar la política `auto()` para que, **si no hay señal cloud Y no hay señal local explícita**, intente **sondear Ollama local** (`is_reachable()` del issue LOCAL1-01, o un ping directo a `/v1/models`): si responde, usar `local_only()` con `embeddinggemma`; si no, caer a `Noop` con un `warn!` claro.
2. El orden de preferencia en `auto()` queda:
   - Explicit `XAVIER_EMBEDDING_PROVIDER_MODE` → respeta.
   - Else if claves cloud presentes → cloud.
   - Else if Ollama local reachable → **local (embeddinggemma)** ← nuevo default inteligente.
   - Else → `Noop` + alerta clara.
3. Log claro al construir: `"Embeddings backend: local-ollama(embeddinggemma) | cloud-openai | disabled(noop)"`.
4. Si el modelo `embeddinggemma` no está instalado en Ollama, `warn!`: `"Modelo embeddinggemma no encontrado. Ejecuta: ollama pull embeddinggemma"`.
5. Tests unitarios de `auto()` con mocks de env vars y de reachability.

## 🔧 Alcance de archivos (aislado)

- `src/embedding/mod.rs` (principal — política `auto()`, `local_only()`)
- Tests en `src/embedding/mod.rs` o nuevo `src/embedding/tests.rs`.

**No tocar:** `src/agents/provider/*` (otro issue), `src/app/proxy_use_case.rs`.

## 🧪 Cómo verificar
```bash
ollama pull embeddinggemma
unset XAVIER_EMBEDDING_PROVIDER_MODE
cargo run -- serve
# log: "Embeddings backend: local-ollama(embeddinggemma)"
curl -s -X POST http://localhost:8006/v1/embeddings -d '{"input":"hola"}' | jq '.data[0].embedding | length'
```

## 📎 Dependencias
- Ideal (no bloqueante) el issue LOCAL1-01 (`is_reachable()`) para reusar el sondeo.
EOF

cat > "$DIR/05.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Xavier ya tiene un modo de embeddings "GLLM" pensado para correr **en GPU local sin proceso externo** (`ProviderMode::LocalGllm`, `src/embedding/mod.rs:50`). El archivo `src/embedding/gllm.rs` existe (111 líneas) pero su madurez/estado es incierto y no hay tests ni docs de cómo usarlo. Para la meta 100% local (especialmente en máquinas con GPU), GLLM es el camino al embedding sin Ollama.

## 📋 Problema

- `src/embedding/gllm.rs` (111 líneas) — revisar si está completo o es stub.
- No hay tests visibles para el backend GLLM.
- No hay documentación de qué modelo/dimensión usar ni cómo activarlo.

## ✅ Criterio de aceptación

1. **Auditoría**: revisar `src/embedding/gllm.rs` completo. Determinar si: carga un modelo en-proceso (¿`candle`? ¿`ort`?) o también es HTTP; qué dependencias de Cargo requiere.
2. Si GLLM ya funciona: añadir tests de integración (marcados `#[ignore]` por requerir GPU/modelo) y documentar modelo+dimensión recomendados.
3. Si GLLM es un stub incompleto: **documentar claramente su estado** (`//! TODO` en el archivo + nota en `docs/`) y dejarlo fuera del path por defecto, pero asegurando que `ProviderMode::LocalGllm` no panic sino que devuelva `Err` claro.
4. En `EmbedderConfig::gllm_only()`, validar que el modelo exista antes de construir; si no, `Err("GLLM backend requires model at X; set XAVIER_GLLM_MODEL_PATH")`.
5. Documentar en `docs/LOCAL_EMBEDDINGS.md` (nuevo) el modo GLLM vs modo Ollama, con tabla comparativa.

## 🔧 Alcance de archivos (aislado)

- `src/embedding/gllm.rs`
- `src/embedding/mod.rs` **sólo** la función `gllm_only()` y el match de `LocalGllm` (no tocar `auto()` que es el issue LOCAL1-04).
- `docs/LOCAL_EMBEDDINGS.md` (nuevo).

## 🧪 Cómo verificar
```bash
cargo test -p xavier embedding::gllm
XAVIER_EMBEDDING_PROVIDER_MODE=local-gllm cargo run -- serve
```

## 📎 Dependencias
Ninguna bloqueante. Paralelo a LOCAL1-04 (ambos tocan `mod.rs` pero funciones distintas: `gllm_only()` vs `auto()`).
EOF

cat > "$DIR/06.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

La configuración actual está fragmentada y contradictoria:
- `config/xavier.config.json` tiene `provider: "local"`, `local_llm_url`, `local_llm_model: "qwen3-coder"`, modelos router `opencode/*` sin claves.
- `.env` **no tiene ninguna API key cloud** ni las vars de embedding.
- `.env.example` lista claves cloud pero no documenta las **vars de modo local**.

Para 100% local, necesitamos una config coherente, documentada y con valores por defecto sensatos.

## 📋 Problema
No existe una configuración "funciona out-of-the-box" 100% local. Un usuario nuevo que instale Ollama no sabe qué vars poner.

## ✅ Criterio de aceptación

1. **`config/xavier.config.json`**: revisar el bloque `models` para que los defaults sean coherentes 100% local:
   - `provider: "local"`, `local_llm_url`, `local_llm_model` (chat).
   - Bloque `embeddings` nuevo: `provider_mode: "local"`, `model: "embeddinggemma"`, `endpoint`.
   - Quitar/quitar a `null` los `opencode/*` del router si no hay claves.
2. **`.env.example`**: añadir sección comentada `# --- LOCAL-FIRST (Ollama) ---` con todas las vars relevantes: `XAVIER_MODEL_PROVIDER=local`, `XAVIER_LOCAL_LLM_URL`, `XAVIER_LOCAL_LLM_MODEL`, `XAVIER_EMBEDDING_PROVIDER_MODE=local`, `XAVIER_EMBEDDING_MODEL=embeddinggemma`, `OLLAMA_HOST`.
3. **`docs/LOCAL_SETUP.md`** (nuevo): guía paso a paso para 100% local: instalar Ollama → `ollama pull qwen3-coder embeddinggemma` → copiar `.env.example` → `xavier serve`. Incluir troubleshooting (puerto 11434 ocupado, modelo no encontrado).
4. Validar que la config parsea sin error con un test que cargue `xavier.config.json`.

## 🔧 Alcance de archivos (aislado)
- `config/xavier.config.json`
- `.env.example`
- `docs/LOCAL_SETUP.md` (nuevo)
- Posible test de carga de config.

**No tocar:** código Rust de providers/embeddings (otros issues).

## 🧪 Cómo verificar
```bash
cp .env.example .env
ollama pull qwen3-coder && ollama pull embeddinggemma
cargo run -- serve  # debería log: local LLM + local embeddings
```

## 📎 Dependencias
Ninguna bloqueante. Documenta el resultado de los issues LOCAL1-01 a LOCAL1-04.
EOF

cat > "$DIR/07.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Existe un setup wizard CLI (`src/cli/handlers/setup.rs:8 handle_setup`, `save_config:118`) que ya escanea Ollama (`src/cli/handlers/setup.rs:19`). Pero el flujo asume configuración cloud y no ofrece un camino guiado "no tengo claves, quiero todo local" en un paso.

## 📋 Problema
Un usuario que quiera 100% local debe configurar manualmente varias vars. Falta un `xavier setup --local` que en un comando: detecte Ollama, sugiera `ollama pull` de los modelos necesarios, y escriba `.env` + `xavier.config.json` para modo local.

## ✅ Criterio de aceptación

1. Añadir flag `--local` (o subcomando) a `xavier setup` que ejecute un flujo guiado:
   - Detectar Ollama (usar `SystemScan`). Si no corre: imprimir instrucciones de instalación + salir con código claro.
   - Si Ollama corre pero faltan `qwen3-coder` y/o `embeddinggemma`: ofrecer ejecutar `ollama pull` (con confirmación `y/N`).
   - Probar reachability del LLM (chat simple) y del embedder (un embedding de prueba).
   - Escribir `.env` (sección local-first) y `config/xavier.config.json` con `provider: local`.
   - Mostrar resumen: `✅ Xavier 100% local listo. LLM: qwen3-coder | Embeddings: embeddinggemma`.
2. Si no se pasa `--local`, mantener el flujo existente sin cambios.
3. El wizard debe ser **idempotente**: re-ejecutar no rompe nada.
4. Tests del flujo con Ollama mockeado (mockito).

## 🔧 Alcance de archivos (aislado)
- `src/cli/handlers/setup.rs` (principal)
- `src/cli/commands/enums.rs` o `src/cli/commands/mod.rs` **sólo** para añadir el flag/subcomando (no tocar otros comandos).
- `src/cli/onboarding.rs` (reusar `SystemScanner`).

## 🧪 Cómo verificar
```bash
ollama serve &
cargo run -- setup --local
```

## 📎 Dependencias
- Conveniente LOCAL1-02 (`SystemScan` cacheado) y LOCAL1-06 (config schema).
EOF

cat > "$DIR/08.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Los endpoints headless `/v1/providers` (`src/cli/handlers/headless_api.rs:144`), `/v1/provider/status` (`:171`), `/v1/quota` (`:199`), `/v1/agents` (`:255`) y `/v1/agents/spawn` (`:270`) devuelven **JSON hardcodeado** (múltiples `AxumJson(json!({...}))` en `:164,:172,:186,:200,:226,:256,:275`). Cualquier UI/dashboard que los consuma muestra **estado falso**: siempre reporta `anthropic+openai` "ok" aunque no haya claves.

## 📋 Problema
Un dashboard de providers que muestre "todo verde" cuando en realidad no hay ningún proveedor usable es un bug serio de observabilidad. Para 100% local, el status real debe reflejar: ¿hay Ollama? ¿qué modelos? ¿está en la fallback chain?

## ✅ Criterio de aceptación

1. `headless_providers` (`:144`): devolver lista real de proveedores desde `CliState.provider_router` (o `get_all_configured()`) con campos: `name`, `mode` (cloud/local), `configured`, `reachable` (LOCAL1-01), `in_fallback_chain`.
2. `headless_provider_status` (`:171`): devolver el proveedor **activo** actual + la cadena de fallback completa (LOCAL1-03).
3. `headless_quota` (`:199`): devolver el estado de rate-limit real desde `rate_manager` (no inventar números).
4. `headless_agents` (`:255`) y `headless_spawn` (`:270`): revisar si deben mapear a agentes reales; si son stub, devolver `501 Not Implemented` + mensaje claro en vez de JSON fake.
5. **No inventar datos.** Si no hay info, devolver `null` o array vacío, nunca un mock "ok".
6. Tests: con provider router vacío, los endpoints devuelven arrays vacíos (no fakes).

## 🔧 Alcance de archivos (aislado)
- `src/cli/handlers/headless_api.rs` (los handlers `:144` a `:330`)
- Posible lectura de `CliState` (campos que ya existen tras LOCAL1-03).

**No tocar:** rutas de chat, `proxy_use_case.rs`.

## 🧪 Cómo verificar
```bash
curl -s http://localhost:8006/v1/providers | jq
# sin claves cloud: []. Con Ollama: [{name:"local", reachable:true, in_fallback_chain:true}]
```

## 📎 Dependencias
- Conveniente LOCAL1-03 (fallback chain en el router) para tener datos reales.
EOF

cat > "$DIR/09.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

La capa de **almacenamiento vectorial** de Xavier ya es 100% local: SQLite + la extensión `sqlite-vec` (`src/memory/sqlite_vec_store/` — `mod.rs`, `vector.rs`, `search.rs`, `graph.rs`, `types.rs`). No depende de ningún servicio cloud de vectores. Esto es una **ventaja clave** para la meta 100% local, pero **no está documentado como decisión arquitectónica**.

Hay precedente de ADR en `docs/ADR/` (001 a 005) y un devlog relevante `docs/devlog/2026-05-10-why-sqlite-vec.md`.

## 📋 Problema
Falta un ADR que registre: por qué SQLite-Vec (vs Qdrant/Milvus/Pinecone), cómo encaja con los embeddings locales, y qué implica para operar 100% offline.

## ✅ Criterio de aceptación

1. Crear `docs/ADR/006-vector-store-local-sqlite-vec.md` con estructura de ADR:
   - **Contexto**: necesidad de almacenar/recuperar embeddings.
   - **Decisión**: SQLite + sqlite-vec, local, sin servicio externo.
   - **Alternativas consideradas**: Qdrant, Milvus, Pinecone, pgvector, in-memory.
   - **Consecuencias**: (+) zero-dependency externo, file-portable, backup simple; (−) escala limitada vs DB dedicado, single-writer.
   - **Mapeo al código**: referenciar `src/memory/sqlite_vec_store/` y `vec-store.sqlite3`.
2. Actualizar `docs/ADR/README.md` (o INDEX) para listar el 006.
3. Actualizar `docs/devlog/2026-05-10-why-sqlite-vec.md` con cross-link al ADR nuevo.
4. Añadir una sección "Vector Store" a la futura `docs/LOCAL_SETUP.md` (LOCAL1-06) — o referenciar desde ahí.

## 🔧 Alcance de archivos (aislado)
- `docs/ADR/006-vector-store-local-sqlite-vec.md` (nuevo)
- `docs/ADR/README.md` o índice de ADRs.
- `docs/devlog/2026-05-10-why-sqlite-vec.md` (sólo cross-link).

**No tocar:** código Rust (issue puramente documental).

## 🧪 Cómo verificar
Revisión de pares: el ADR debe responder "si quiero correr Xavier sin internet, ¿la memoria semántica funciona?" → sí.

## 📎 Dependencias
Ninguna. Documenta fundación existente.
EOF

cat > "$DIR/10.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

El backend de embeddings local (Ollama + `embeddinggemma`) está soportado (`src/embedding/openai.rs` con `DEFAULT_LOCAL_EMBEDDING_ENDPOINT`) pero **no hay tests de integración** que verifiquen el flujo completo: texto → embedding → dimensión correcta → almacenamiento → búsqueda. Para garantizar 100% local sin regresiones, necesitamos una suite que se pueda correr contra Ollama real (o mockeado).

## 📋 Problema
Sin tests, cualquier cambio en `src/embedding/` o en el cliente HTTP puede romper silenciosamente los embeddings locales. No hay forma automática de verificar que `embeddinggemma` produce vectores de la dimensión esperada y que se almacenan/recuperan bien.

## ✅ Criterio de aceptación

1. Crear `tests/embedding_local_integration.rs` (test de integración):
   - Si `XAVIER_TEST_OLLAMA=1` (o si Ollama reachable): correr contra Ollama real.
   - Si no: usar `mockito` para simular `/v1/embeddings` con respuesta fake de dimensión N.
2. Tests a cubrir:
   - `encode("hola")` devuelve `Vec<f32>` no vacío con dimensión == `config.dimension`.
   - Dos textos similares tienen cosine similarity > umbral (sanity).
   - El `FallbackEmbedder` salta al siguiente backend si el primero falla.
   - `NoopEmbedder` devuelve zero-vector y `dimension()` consistente.
3. Test de la cadena: texto → embedder → `sqlite_vec_store` insert → búsqueda por similitud devuelve el correcto (mock del embedder con vectores deterministas).
4. Documentar en `docs/LOCAL_EMBEDDINGS.md` cómo correr: `XAVIER_TEST_OLLAMA=1 cargo test --test embedding_local_integration`.

## 🔧 Alcance de archivos (aislado)
- `tests/embedding_local_integration.rs` (nuevo)
- Posible fichero de helpers de test compartido en `tests/common/` (si existe).
- `docs/LOCAL_EMBEDDINGS.md` (sección tests — coordinar con LOCAL1-05).

**No tocar:** `src/embedding/*.rs` (sólo tests externos).

## 🧪 Cómo verificar
```bash
ollama pull embeddinggemma
XAVIER_TEST_OLLAMA=1 cargo test --test embedding_local_integration
# o sin Ollama:
cargo test --test embedding_local_integration
```

## 📎 Dependencias
- Conveniente LOCAL1-04 (default local) y LOCAL1-05 (gllm) mergeados.
EOF

cat > "$DIR/11.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Existe `src/embedding/cache.rs` (módulo de cache de embeddings). En modo 100% local, los embeddings vía Ollama son gratis pero **no instantáneos** (latencia de inferencia por token). Un cache efectivo reduce llamadas repetidas a Ollama y acelera la indexación de memoria y el code-graph.

## 📋 Problema

- Revisar `src/embedding/cache.rs`: ¿es in-memory? ¿persiste a disco? ¿Invalida al cambiar de modelo de embedding (dimensión distinta)?
- Para 100% local, idealmente el cache persiste en SQLite (junto al vector store) y se invalida por modelo.

## ✅ Criterio de aceptación

1. **Auditoría** de `src/embedding/cache.rs`: documentar qué hace hoy (capacidad, política de evicción, persistencia).
2. Garantizar que el cache está **keyado por modelo de embedding** (no sólo por texto): `{model, text_hash}` → vector. Así cambiar de `embeddinggemma` a otro no sirve vectores de dimensión incorrecta.
3. Añadir persistencia opcional a disco (tabla SQLite `embedding_cache` o archivo mmap) activable vía env `XAVIER_EMBEDDING_CACHE_PERSIST=1`.
4. Métricas: hits/misses logueados a nivel `debug`.
5. Tests unitarios: hit tras insert, miss al cambiar modelo, evicción por capacidad.
6. **No cambiar la API pública** del trait `Embedder` — el cache debe ser transparente (wrapper).

## 🔧 Alcance de archivos (aislado)
- `src/embedding/cache.rs` (principal)
- Posible tabla SQLite nueva en `src/memory/sqlite_vec_store/` **sólo si** añades persistencia (coordinar nombre de tabla con LOCAL1-09 — pero son archivos distintos).
- Tests en `src/embedding/cache.rs`.

**No tocar:** `src/embedding/mod.rs::auto()` (LOCAL1-04), `gllm.rs` (LOCAL1-05).

## 🧪 Cómo verificar
```bash
cargo test -p xavier embedding::cache
```

## 📎 Dependencias
Ninguna bloqueante. Paralelo a LOCAL1-04/05.
EOF

cat > "$DIR/12.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Cuando Xavier esté 100% local, el operador necesita visibilidad clara del estado: ¿estoy en modo local? ¿Ollama está vivo? ¿qué modelo de chat y de embedding se están usando? Hoy los logs son dispersos y no hay una "salud de modo local" agregada. Existe `src/server/alerts.rs` (referenciado desde `src/embedding/mod.rs:157` como `SYSTEM_ALERTS.push_alert`).

## 📋 Problema
No hay un punto único que responda "¿Xavier está funcionando 100% local y sano?". Los fallos (Ollama caído, modelo no encontrado) se loguean pero no se agregan en un status de salud legible.

## ✅ Criterio de aceptación

1. Revisar `src/server/alerts.rs`: exponer un snapshot de alertas activas + un "modo de operación" derivado: `local-healthy`, `local-degraded` (Ollama caído), `cloud-fallback`, `disabled`.
2. En el arranque del servidor, emitir **un** log de resumen claro:
   ```
   🟢 Xavier iniciado — modo: LOCAL
      LLM:        ollama/qwen3-coder @ localhost:11434 [reachable]
      Embeddings: ollama/embeddinggemma @ localhost:11434 [reachable]
      Vector DB:  sqlite-vec (.xavier/vec-store.sqlite3)
   ```
   o 🔴 si algo falta.
3. Hook de health-check: endpoint `/health` (o el existente) debe incluir `mode`, `llm`, `embeddings`, `vector_db` con status por componente.
4. Alerta proactiva: si Ollama pasa de reachable a unreachable en runtime (detectado por fallos consecutivos), emitir `SYSTEM_ALERTS.push_alert("ERROR", "Ollama local no responde — modo degradado", "llm")`.
5. Tests del derivador de modo.

## 🔧 Alcance de archivos (aislado)
- `src/server/alerts.rs` (principal)
- `src/cli/handlers/headless_api.rs:22 headless_health` **sólo** para enriquecer la respuesta (dividir con LOCAL1-08: #08 hace providers/quota/agents, este hace health).
- Boot logging en `src/cli/server.rs` (sólo la línea de resumen, no rutas).

## 🧪 Cómo verificar
```bash
curl -s http://localhost:8006/health | jq '.mode, .llm, .embeddings'
```

## 📎 Dependencias
- Conveniente LOCAL1-01 (is_reachable) y LOCAL1-02 (SystemScan).
EOF

cat > "$DIR/13.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Xavier soporta ejecutar el LLM localmente vía el CLI `opencode` como subprocess (`src/agents/provider/client.rs:262 generate_opencode_cli`), además de Ollama. Esto es una vía alternativa de "capa agentic local": en vez de llamar a un endpoint HTTP, Xavier lanza `opencode run --model ...` como proceso. Para la meta 100% local, conviene **documentar y validar** este camino como alternativa cuando Ollama no es viable.

## 📋 Problema

- `generate_opencode_cli` existe pero su estado de salud es incierto (¿requiere `OPENCODE_API_KEY`? ¿es local de verdad o cloud via opencode?).
- No hay docs de cuándo usar opencode-bridge vs Ollama.
- El binario `opencode` puede no estar instalado → error críptico.

## ✅ Criterio de aceptación

1. **Auditar** `src/agents/provider/client.rs:262-300` `generate_opencode_cli`: ¿Es 100% local o `opencode` a su vez llama a cloud? Documentar honestamente. Qué env requiere (`OPENCODE_API_KEY`, `XAVIER_OPENCODE_MODEL`).
2. Crear `docs/LOCAL_LLM_BRIDGES.md` comparando las vías de LLM local:
   | Vía | Proceso externo | 100% offline | Setup |
   |-----|-----------------|--------------|-------|
   | Ollama (default) | sí (ollama serve) | sí | `ollama pull` |
   | LM Studio | sí (servidor) | sí | GUI |
   | opencode CLI bridge | sí (binario opencode) | depende de config | instalar opencode |
3. Validación al arranque: si el proveedor activo es `opencode`, comprobar que el binario existe (`which opencode`); si no, `Err` claro con instrucción de instalación.
4. Documentar el rol de opencode en la futura capa agentic (Ola 2/3): como fallback detrás de Ollama.

## 🔧 Alcance de archivos (aislado)
- `docs/LOCAL_LLM_BRIDGES.md` (nuevo)
- `src/agents/provider/client.rs` **sólo** la validación de binario existente en `generate_opencode_cli` (no tocar el resto del archivo).

**No tocar:** `config.rs`, `proxy_use_case.rs`.

## 🧪 Cómo verificar
```bash
opencode --version 2>/dev/null && XAVIER_MODEL_PROVIDER=opencode cargo run -- serve
```

## 📎 Dependencias
Ninguna. Documenta + endurece una vía existente.
EOF

cat > "$DIR/14.md" <<'EOF'
## 🎯 Contexto recuperado de Xavier

Esta es la iniciativa orquestada para llevar a Xavier a **100% local (LLM + embeddings vía Ollama)**, lanzada en olas de 14 issues paralelos para Jules. Este issue es el EPIC de seguimiento y actualiza los features docs.

## 📋 Problema
No existe una feature trackeada "local-first" ni un EPIC que agrupe los ~42 issues de las 3 olas. El estado actual de features (`.gitcore/features.json` — 22 features, 85% global) no refleja el soporte local.

## ✅ Criterio de aceptación

1. Añadir feature a `.gitcore/features.json`:
   ```json
   {
     "id": "feat-local-first",
     "name": "Local-First LLM + Embeddings (Ollama)",
     "status": "in-progress",
     "progress": 0,
     "description": "Xavier operando 100% offline con LLM (qwen3-coder) y embeddings (embeddinggemma) vía Ollama local, sin claves cloud."
   }
   ```
2. Actualizar `.gitcore/features-detailed.json` con sub-features: `local-llm-chat`, `local-embeddings`, `local-vector-store`, `local-fallback-chain`.
3. Actualizar `architecture.md` tabla de madurez con la feature nueva (~10% inicial).
4. Crear `docs/ROADMAP_LOCAL_FIRST.md` con las 3 olas, sus issues, y un checklist `[ ]` por issue.

## 🔧 Alcance de archivos (aislado)
- `.gitcore/features.json`
- `.gitcore/features-detailed.json`
- `architecture.md` (sólo la tabla de features/madurez)
- `docs/ROADMAP_LOCAL_FIRST.md` (nuevo)

**No tocar:** código Rust (este issue es de tracking/docs).

## 🧪 Cómo verificar
- `.gitcore/features.json` parsea como JSON válido.
- `architecture.md` tabla coherente.

## 📎 Dependencias
Es el paraguas. Se actualiza al final de cada ola con el progreso.
EOF

echo "=== bodies escritos: ==="
ls -la "$DIR"

# ====== Títulos + labels por número ======
declare -a TITLES=(
[01]="Endurecer is_configured() del proveedor local: verificar que Ollama responde"
[02]="Auto-detección de Ollama al arranque del servidor (SystemScan) + lista de modelos"
[03]="ProviderRouter: fallback chain automática que incluya local como último eslabón"
[04]="Hacer Ollama + embeddinggemma el backend de embeddings por defecto sin claves cloud"
[05]="Auditar y completar el backend GLLM (embeddings en GPU local)"
[06]="Config local-first completa: xavier.config.json + .env.example para Ollama local"
[07]="Setup wizard CLI: flujo 100% local guiado (xavier setup --local)"
[08]="Reemplazar JSON mock de /v1/providers /quota /agents por estado real"
[09]="ADR: vector store 100% local (SQLite-Vec) — por qué no Pinecone/etc."
[10]="Suite de tests de integración para embeddings locales vía Ollama"
[11]="Endurecer src/embedding/cache.rs: cache por modelo + persistencia opcional"
[12]="Alertas y observabilidad de modo local: status de salud agregado"
[13]="Documentar el puente opencode CLI como vía alternativa de LLM local"
[14]="EPIC + features.json: iniciativa Xavier 100% Local (LLM + Embeddings vía Ollama)"
)
declare -a LABELS=(
[01]="local-first,llm-local,refactor,jules"
[02]="local-first,llm-local,feat,jules"
[03]="local-first,llm-local,refactor,jules"
[04]="local-first,embeddings-local,feat,jules"
[05]="local-first,embeddings-local,refactor,jules"
[06]="local-first,docs,feat,jules"
[07]="local-first,llm-local,feat,jules"
[08]="local-first,refactor,bug,jules"
[09]="local-first,embeddings-local,docs,jules"
[10]="local-first,embeddings-local,test,jules"
[11]="local-first,embeddings-local,refactor,jules"
[12]="local-first,feat,jules"
[13]="local-first,llm-local,docs,jules"
[14]="local-first,epic,jules"
)

echo ""
echo "=== LANZANDO OLA 1 (14 issues) ==="
for n in 01 02 03 04 05 06 07 08 09 10 11 12 13 14; do
  create "$n" "${TITLES[$n]}" "${LABELS[$n]}" "$DIR/${n}.md"
done

echo ""
echo "=== RESULTADO OLA 1 ==="
cat "$OUT"