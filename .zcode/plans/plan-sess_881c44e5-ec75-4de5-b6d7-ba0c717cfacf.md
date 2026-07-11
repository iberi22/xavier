# Plan: Reparar UI negra + Sistema de logs + Embedding unhealthy

## Diagnóstico (3 problemas confirmados en vivo)

| # | Problema | Causa raíz |
|---|---|---|
| **1** 🔴 | **Panel UI totalmente negro, sin logs ni contenido** | `panel-ui/build/index.html` referencia `/assets/index.js` pero el servidor solo sirve `/panel/assets/*`. El JS/CSS nunca carga → React no monta → solo queda el `<body>` con `background: #050505`. Causa: `vite.config.ts:18` tiene `base: "/"` en lugar de `base: "/panel/"`. |
| **2** 🟡 | **No hay visor de logs en la UI** | `ServiceLogStore` (SQLite+FTS5) y el middleware `request_logger` **están definidos pero dormidos**: nunca se instancian ni se adjuntan al router. No hay endpoint `/logs` ni streaming. Los logs van al disco pero la UI no puede mostrarlos. |
| **3** 🟠 | **`/health` reporta `unhealthy`** | El embedding falla en bucle: pide `gllm` + modelo `Qwen/Qwen3-Embedding-0.6B` (feature no compilado), a pesar de que el `.env` define OpenRouter/OpenAI correctamente. Hay un conflicto de precedencia de config. |

---

## Cambio 1 — Fix UI negra (crítico, ~5 min)

**Archivo:** `panel-ui/vite.config.ts:18`

```diff
- base: "/",
+ base: "/panel/",
```

Esto hace que Vite emita `src="/panel/assets/index.js"` en el build, coincidiendo con la ruta `GET /panel/assets/{*path}` del backend (`src/server/panel/assets.rs:21`).

**Rebuild:** `cd panel-ui && npm run build` (node_modules ya está instalado; `base` se aplica solo al build, no afecta el dev server por el proxy ya configurado).

**Verificación en vivo:** `curl http://localhost:8006/panel` debe devolver HTML con `/panel/assets/...`, y `GET /panel/assets/index.js` ya devuelve 200. La app de React montará y dejará de verse negra.

---

## Cambio 2 — Activar visor de logs (backend + endpoint)

### 2a. Cablear `ServiceLogStore` y `request_logger` en el router

**Archivo:** `src/cli/server.rs` (al final del `start_http_server`, antes de `.with_state()` ~línea 824)

1. Crear el estado de observabilidad:
   ```rust
   let obs_state = Arc::new(observability::ObservabilityState::new());
   ```
2. Adjuntar el middleware al router `app`:
   ```rust
   .layer(middleware::from_fn_with_state(obs_state.clone(), observability::request_logger))
   ```
   Esto persistirá errores 5xx en `service_logs` (SQLite). El logger ya loguea todas las requests vía `tracing`.

### 2b. Crear endpoints REST de logs

**Nuevo archivo:** `src/server/panel/logs.rs` (siguiendo el patrón de `src/server/panel/chat.rs`, `threads.rs`)

Handler functions (registradas en el router público, igual que `/health`):
- `GET /api/logs` — query params: `?level=error&limit=100&source=http_server&q=texto`. Llama a `ServiceLogStore::search_logs()` o un nuevo método `query_filtered()` (filtro por nivel/source + LIMIT, ordenado por timestamp DESC).
- `GET /api/logs/stats` — devuelve `ObservabilityStats` (total, errores última hora, errores hoy, warnings hoy, top módulos con errores) vía `ServiceLogStore::get_stats()`.

Ambos usan `ServiceLogStore::new().await` (zero-argument, toma el `ConnectionManager::global()` ya inicializado). Se protegen con `auth_middleware` (van en `protected_routes`).

**Endpoint de streaming SSE (opcional, fase 2):** `GET /api/logs/stream` — empuja nuevas entradas en vivo. Dado que `EventSource` no permite headers custom, el token va como `?token=...` query param (validado manualmente). *Si prefieres, dejamos el streaming para después y arrancamos solo con polling cada 3s desde la UI, que es más simple y suficiente para ver logs.*

### 2c. Método de query faltante en `ServiceLogStore`

Agregar a `src/observability/service_log.rs` un método `query_recent(limit, level_filter, source_filter)` que retorne las últimas N entradas filtradas (la `search_logs` existente es FTS, no sirve para "últimos logs"). Patrón idéntico al `query_recent_errors` existente.

---

## Cambio 3 — Componente de logs en panel-ui

**Nuevo archivo:** `panel-ui/src/pages/Settings/LogsPage.tsx`
- Modelado en `LeaseHistoryPage.tsx` (timestamped event list con loading/error/empty states y colores por severidad).
- Props: `{ token: string }`, construye `new ApiClient(token)`.
- **Polling** cada 3s a `GET /api/logs?level=...&limit=200` (sin SSE, más simple).
- Filtros: nivel (All/Error/Warn/Info), búsqueda de texto, y barra de stats arriba (errores/hora, top módulos) desde `/api/logs/stats`.
- Colores por convención existente: `text-red-400` error, `text-amber-400` warn, `text-[#39ff14]` info, `text-cyan-400` debug. `font-mono` para las líneas de log.

**Editar:** `panel-ui/src/api/client.ts` — añadir métodos `getLogs(filters)` y `getLogStats()` a `ApiClient`.

**Editar:** `panel-ui/src/components/ConfigModal.tsx` — añadir tab "Logs":
- Añadir `"logs"` al `MainTab` union (línea ~49).
- Añadir `<TabButton>` en la barra de tabs (~línea 146).
- Añadir bloque de render condicional pasando `token={token || ""}` (~línea 324).

---

## Cambio 4 — Arreglar embedding unhealthy

**Investigar primero** `src/embedding/mod.rs:156` (de dónde sale el default `gllm` + `Qwen/Qwen3-Embedding-0.6B`). El `.env` define correctamente:
```
XAVIER_EMBEDDING_PROVIDER_MODE=cloud
XAVIER_EMBEDDING_URL=https://openrouter.ai/api/v1/embeddings
XAVIER_EMBEDDING_MODEL=text-embedding-3-small
XAVIER_EMBEDDING_API_FLAVOR=openai
```
Pero en runtime pide `gllm`. Hipótesis: el resolvedor de config da prioridad a `config/xavier.config.json` (campos vacíos) sobre el `.env`, y al estar vacíos cae en un default hardcoded de `gllm`+`Qwen`.

**Fix (a confirmar al leer el código):** una de:
- (a) Forzar precedencia: `.env` > `config.json` > defaults en el resolvedor de embedding.
- (b) Rellenar `config.json` con los valores cloud correctos (endpoint, embedder, model) para que no caiga en defaults.
- (c) Si hay un estado persistido obsoleto (data/ o .xavier/), limpiar el override.

**Verificación:** tras reiniciar el server, `curl /health` debe reportar `embedding.status: "healthy"`.

---

## Orden de ejecución y verificación

1. **Cambio 1 (UI negra)** — aplicar fix vite + `npm run build` + verificar `/panel` carga React. *Impacto inmediato y visible.*
2. **Cambio 4 (embedding)** — leer `embedding/mod.rs:156`, aplicar fix de precedencia/config, reiniciar, verificar `/health` healthy.
3. **Cambio 2 (backend logs)** — cablear `ServiceLogStore` + middleware + endpoints `/api/logs` y `/api/logs/stats`. Compilar (`cargo build`).
4. **Cambio 3 (UI logs)** — crear `LogsPage.tsx`, métodos de `ApiClient`, tab en `ConfigModal`. Rebuild panel-ui.
5. **Rebuild Rust** (`cargo build --release`) y reiniciar servidor para activar endpoints.
6. **Verificación final:** abrir `http://localhost:8006/panel`, confirmar UI cargada, health verde, tab Logs funcional mostrando entradas reales.

## Notas
- **No se añade lib nueva:** todo reusa `tracing` (ya en uso), `ServiceLogStore` (ya construido), `ApiClient` (ya en uso), y patrones de componentes existentes.
- **Streaming SSE:** se deja como polling (3s) en la fase 1 por simplicidad y robustez. Si quieres streaming en vivo real, se puede añadir `/api/logs/stream` después.
- El `base: "/panel/"` no afecta el dev server de Vite (sigue en `127.0.0.1:4174` con su proxy).
- Los endpoints de logs irán protegidos por `auth_middleware` (token `X-Xavier-Token`), consistente con `/panel/api/*`.