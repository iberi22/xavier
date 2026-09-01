# PLAN — Xavier WAVE-5 Browser Compat + Versioning — 2026-09-01

> **Estado:** RECOLECCION COMPLETA — listo para crear ola de 10 issues
> **Preflight:** READY (0.0.1 alineado, 52/52 stable, GH 5000/5000, 90/500 turns)
> **Dictamen base:** Panel-UI browser compat (P0/P1/P2/P3) + gaps de ecosistema recolectados

## Recoleccion de gaps — todo lo que falta

### A. Versionado (ya mitigado, falta automatizar)

- ✅ Revert v1.0.0 + alinear manifests + VERSIONING.md + swal-preflight CLI/skill — hecho 01sep
- ❌ Falta: CI check `version-sync` (fail si Cargo/tauri/package desalineados)
- ❌ Falta: `swal-preflight preflight` como gate en CI antes de crear ola
- ❌ Falta: ADR firmada para gate 1.0.0 + CHANGELOG Unreleased ya listo pero sin CI que lo exija

### B. Panel-UI browser compat — audit vivo 01sep (post commit 8abf095d)

**Build:** `pnpm build` PASS (vite 8, 3647 modules, 1.16MB), pero runtime crashea en browser.

**Scan `invoke`/`listen` sin guard (rg):**

| Archivo | Linea | Llamada | Guard? | Impacto real hoy |
|---------|-------|---------|--------|-----------------|
| TopStatusBar.tsx | 1 | `import {invoke}` static | — | throw en browser |
| TopStatusBar.tsx | 43 | `invoke("get_xavier_token")` | try/catch → localStorage vacio | token="" → 401 |
| TopStatusBar.tsx | 95 | `invoke("get_current_config_state")` | ❌ sin guard | crash |
| TopStatusBar.tsx | 102 | `invoke("get_realtime_metrics")` | ❌ sin guard | crash → metrics 0 |
| TopStatusBar.tsx | 152 | `listen("new-notification")` | ❌ sin guard | transformCallback crash |
| NotificationCenter.tsx | 1-2 | static imports | — | throw |
| NotificationCenter.tsx | 79 | `invoke("get_xavier_token")` | try/catch | 401 |
| NotificationCenter.tsx | 273 | `listen` | ❌ | crash |
| NotificationsDropdown.tsx | 1-2 | static imports | — | throw |
| NotificationsDropdown.tsx | 77 | `invoke("get_xavier_token")` | try/catch | 401 |
| NotificationsDropdown.tsx | 184 | `listen` | ❌ | crash |
| App.tsx | 1 | static import | — | throw (pero checkNativeConfig tiene guard) |
| App.tsx | 184 | `invoke("get_current_config_state")` | ✅ dentro `if isTauri` | OK, pero falta else HTTP |
| InputArea.tsx | 1-2 | static imports | — | throw |
| InputArea.tsx | 53 | `invoke("scan_project_folder")` + `open({directory:true})` | ❌ | crash picker |

**Parcialmente mitigado en 8abf095d:** App.tsx guard + normalizeMessage + AuthProvider API_TOKEN fallback. Insuficiente.

**Endpoints backend verificados (con token de .env.local len 32):**

- `GET /health` → 200 sin auth, `system:{cpu_usage, ram_usage_percent}` ✓
- `GET /v1/system/info` → 200 con token `{cpus,memory_mb,arch}` ✓
- `GET /notifications` → 200 con token (21KB array) ✓
- `GET /v1/memories?limit=1` → 200 ✓
- `GET /v1/config` → 404 ❌ (no existe; correcto es no usarlo — usar `hasConfig=true` local o `/v1/config/providers`)
- `GET /panel/api/config` → 404 ❌

**Faltantes dictamen no implementados:**

- `useApiToken` hook centralizado (7)
- `LoadingSpinner` reutilizable (8)
- `ErrorToast` global (9)
- Polling fallback 30s para notifs cuando no hay Tauri
- Métricas via `/health` en lugar de `get_realtime_metrics`
- Config via http fallback en lugar de `get_current_config_state`
- Folder picker browser File API (`webkitdirectory`)
- Manejo 400 en `refreshSession` preservando API_TOKEN
- Error boundary + skeletons + loading states

### C. Xavier runtime gaps (fuera de panel)

- Health `degraded` por mesh (1 peer, libp2p 10%, sync_lag 218k secs) — no bloquea pero debe documentarse
- pnpm field `pnpm.overrides` deprecated (warning) — migrar a `pnpm.overrides` en pnpm-workspace?
- vite `__APP_VERSION__` usa `0.6.1-beta` fallback, no lee Cargo version
- No E2E browser test en CI (playwright config existe pero no corre en GH Actions con token)
- No CI que inyecte `VITE_XAVIER_API_TOKEN` en build

### D. Docs/SRS/GitCore

- SRS 43 REQs, FEATURE_STATUS 52/52 stable — OK
- Falta REQ para browser-compat (nuevo REQ-044?)
- Falta ADR browser-safe pattern + ADR versioning gate (VERSIONING.md ya canon pero sin ADR)

---

## Plan de ola — 10 issues (prioridad P0→P3)

Formato: cada issue `feat-`/fix con template canonico 11 secciones Rust (skill xavier-jules-wave).

### P0 — Critico (crasheos)

**#1 — feat(panel): hook centralizado `useApiToken` browser-safe**
- Crear `src/hooks/useApiToken.ts`: `storeToken ?? VITE_XAVIER_API_TOKEN ?? ""`
- Reemplazar todos `getAuthToken()` (4 archivos) con hook o `import.meta.env` directo en helpers fuera de React
- Eliminar `import {invoke} from "@tauri-apps/api/core"` static donde solo se usaba para token
- Archivos: TopStatusBar, NotificationCenter, NotificationsDropdown (y via hook los demas)
- Verif: `rg "get_xavier_token" panel-ui/src` == 0, `pnpm build` PASS

**#2 — fix(panel): TopStatusBar guards + /health metrics + config fallback**
- Guard `isTauri = "__TAURI_INTERNALS__" in window` + dynamic `import("@tauri-apps/api/core")` solo en rama Tauri
- `get_realtime_metrics` → `fetch("http://127.0.0.1:8006/health")` sin auth, mapear `system.cpu_usage` / `ram_usage_percent`
- `get_current_config_state` → `fetch(getApiUrl("/v1/config/providers"), {X-Xavier-Token})` o simplemente `setHasConfig(true)` (LLM local siempre)
- `listen("new-notification")` → if isTauri dynamic import else `setInterval(fetchMetrics, 30000)` + cleanup
- Loading state `isLoading` + spinner mientras `fetchMetrics` pendiente
- Archivos: TopStatusBar.tsx

**#3 — fix(panel): NotificationCenter + NotificationsDropdown browser-safe + polling**
- Misma receta: token via `VITE_XAVIER_API_TOKEN`, `listen` con guard + `setInterval(fetchNotifications, 30000)` fallback
- Endpoints: `GET /notifications` con `X-Xavier-Token: getToken()`, `PATCH /notifications/{id}/read` y `/read-all`
- Skeletons: 3 divs `animate-pulse` mientras `isLoading`
- Cleanup intervals en `useEffect return`
- Archivos: NotificationCenter.tsx, NotificationsDropdown.tsx

### P1 — Alto

**#4 — fix(panel): InputArea folder picker browser File API**
- `open({directory:true})` + `invoke("scan_project_folder")` → guard isTauri ? Tauri : browser `<input type="file" webkitdirectory style display:none>` + `file.webkitRelativePath.split("/")[0]`
- Mantener compat Tauri con dynamic import
- Archivo: InputArea.tsx

**#5 — fix(panel): App.tsx config state via HTTP fallback**
- Ya tiene guard, agregar `else { fetch(getApiUrl("/v1/config/providers"), {headers:{X-Xavier-Token: currentToken}}) → setHasConfig(true) }`
- Si 404, fallback a `hasConfig=true` (Ollama local)
- Archivo: App.tsx

**#6 — fix(auth): AuthProvider refresh 400 preserva API_TOKEN**
- `refreshSession catch`: `set({user:null, token: API_TOKEN, refreshToken:null, isAuthenticated:false})` — no limpiar API_TOKEN
- No loop infinito: solo 1 intento, si 400 → re-login operador pero panel sigue con master key
- Archivo: AuthProvider.tsx

### P2 — Medio

**#7 — feat(panel): LoadingSpinner + skeletons + ErrorToast**
- `src/components/ui/LoadingSpinner.tsx` (svg animate-spin, size prop, stroke #10b981)
- `src/components/ui/ErrorToast.tsx` (fixed bottom-left, 4s auto-dismiss, ⚠️, border amber/red)
- Usar Spinner en TopStatusBar, NotificationsDropdown, ChatHistory; Skeletons en notifs
- Nuevos archivos + integracion

**#8 — chore(panel): vite __APP_VERSION__ + pnpm overrides + E2E browser gate**
- `vite.config.ts`: `__APP_VERSION__` lee `Cargo.toml` version (0.0.1) en lugar de `0.6.1-beta` hardcode
- `package.json`: mover `pnpm.overrides` a `pnpm-workspace.yaml` o eliminar warning
- CI: agregar job `panel-browser-smoke` que hace `pnpm build` con `VITE_XAVIER_API_TOKEN` dummy y verifica `grep -c token dist/assets/*.js >=1`
- Verif: `pnpm build` sin warnings relevantes

### P3 — Bajo + Versioning

**#9 — chore(ci): version-sync check + preflight gate**
- `scripts/check-version-sync.sh` que corre `swal-preflight check` y falla si `versions desalineadas` o sin `[Unreleased]`
- `.github/workflows/ci.yml` agrega step `swal-preflight preflight --wave 10 --json` como gate pre-wave
- Docs: `docs/SWAL_VERSIONING.md` ya canon, agregar badge version en README que lee Cargo

**#10 — docs(adr): ADRs browser-safe + versioning gate + SRS REQ-044**
- `docs/adr/ADR-XXX-panel-browser-compat.md` (contexto Tauri→browser, decision guard+polling, consecuencias)
- `docs/adr/ADR-XXX-swal-versioning.md` (semver 0.y.z gate, conventional commits)
- `docs/SRS/REQUIREMENTS.md`: nuevo `REQ-044: Panel browser compat` con criterios (no invoke sin guard, /health fallback, polling)
- `docs/plans/PLAN_WAVE5_BROWSER_COMPAT_2026-09-01.md` (este archivo) como referencia

---

## Secuencia y routing

- Estimacion 10 issues: 380k tokens, 90 turns / 500 → 1 sesion ok, $1.90
- Routing: #1-7 impl directa → hermes/muse-spark-1.2 (opencode-go, 1M ctx)
- #8, #9 CI/docs → hermes alta o agy research si paralelizas
- Orden: #1 primero (desbloquea token para #2,#3,#5), luego #2+#3 en paralelo, #4-6 paralelo, #7, #8-10 cierre

## Verificacion final (definicion de done)

```bash
cd ~/proyectosSWAL/apps/xavier/panel-ui && pnpm build  # PASS, 3647 modules
grep -r "invoke(\"get_xavier_token\"" panel-ui/src # 0 resultados
grep -r "from \"@tauri-apps/api" panel-ui/src | grep -v "await import" # solo dynamic
grep -c "VITE_XAVIER_API_TOKEN" dist/assets/index.js # >=1
curl -H "X-Xavier-Token: $TOKEN" http://127.0.0.1:8006/notifications | jq length # 200
# En browser http://127.0.0.1:8006/ : sin TypeError invoke/transformCallback, TopStatusBar CPU/RAM ok, notifs vacias sin 401 loop, chat "hola" responde, no polling 401 infinito
swal-preflight check --cwd ~/proyectosSWAL/apps/xavier # sync ok, Unreleased ok, clean
```

## Estado pre-wave

- ✅ Versioning done + preflight READY (94 commits since v0.0.1, clean)
- ⏳ Esperando creacion de 10 issues con este plan — no crear hasta OK usuario
- Siguiente bump tras wave: `0.0.1 → 0.1.0` via `swal-preflight bump --to 0.1.0`
