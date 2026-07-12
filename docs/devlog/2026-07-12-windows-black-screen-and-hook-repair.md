# Windows Black Screen Fix & Pre-Push Hook Repair

**Date**: 2026-07-12
**Author**: Xavier AI
**Tags**: [panel-ui, tauri, windows, security, ci, hooks, tech-debt]
**Source Files**: [`panel-ui/src/App.tsx`](file:///e:/scripts-python/xavier/panel-ui/src/App.tsx), [`src/security/auth.rs`](file:///e:/scripts-python/xavier/src/security/auth.rs), [`src/auth2/mod.rs`](file:///e:/scripts-python/xavier/src/auth2/mod.rs), [`.githooks/pre-push`](file:///e:/scripts-python/xavier/.githooks/pre-push), [`ci-local.ps1`](file:///e:/scripts-python/xavier/ci-local.ps1)
**Commits**: `8fc71bb7`, `8e5f9fa2`, `335b1751`, `3cd41858`, `552300ad`, `a1c59c4f`, `7a046656`
**Issues**: [#476](https://github.com/iberi22/xavier/issues/476), [#477](https://github.com/iberi22/xavier/issues/477), [#478](https://github.com/iberi22/xavier/issues/478)

---

## TL;DR
La app de escritorio de Xavier en Windows (Tauri) mostraba **pantalla negra** tras el login. La causa raíz fue un bug preexistente en `panel-ui/src/App.tsx`: el componente invocaba `setThreads(...)` en cuatro lugares pero **el estado `threads` nunca fue declarado** con `useState`, provocando un `ReferenceError` que crasheaba React antes de montar el árbol DOM. Al diagnosticar el fix, se reparó también el `pre-push` hook de git (roto por un shebang `pwsh` ausente) y se descubrieron tres ítems de deuda técnica que ahora están trackeados como issues.

---

## Context & Motivation

El panel UI de Xavier es una app Tauri 2.11 + React 19 que empaqueta el frontend en una ventana WebView2 nativa de Windows. El backend Rust (`xavier.exe`) se lanza como sidecar y sirve la API HTTP en `127.0.0.1:8006`.

```
┌─────────────────────────────┐
│  Tauri WebView2 (app.exe)   │  ← React SPA
│  React 19 + Vite            │
└──────────┬──────────────────┘
           │ HTTP (127.0.0.1:8006)
           ▼
┌─────────────────────────────┐
│  Backend Rust (xavier.exe)  │  ← sidecar
│  Axum + Tokio + SQLite-Vec  │
└─────────────────────────────┘
```

El síntoma era inequívoco: la ventana abría en negro y no respondía. El backend sidecar corría bien (puerto 8006 LISTENING, `/health` y `/panel` devolvían 200), así que el fallo estaba **en el render del frontend**, no en el transporte.

---

## The Decision

La depuración siguió tres frentes que se descubrieron encadenados:

1. **Fix de pantalla negra** — declarar el estado `threads` faltante en `App.tsx`.
2. **Endurecimiento de seguridad** — redactar secrets en los trait `Debug` de `User` y `LoginRequest`.
3. **Reparación del `pre-push` hook** — que al intentar validarlo reveló deuda técnica estructural (crates rotos en el workspace).

---

## Deep Dive: Technical Implementation

### 1. La pantalla negra: `setThreads` sin `useState`

`App.tsx` es el componente raíz. Un usuario autenticado dispara este flujo al montar:

```tsx
useEffect(() => {
  if (!token) return;
  void loadThreads(token);   // ← dispara setThreads(...)
  void loadPanelData(token);
}, [token, loadThreads, loadPanelData]);
```

`loadThreads` hace `fetch` y luego `setThreads(data)`. El problema: `setThreads` **no existía** como binding de `useState`. Se invocaba en cuatro sitios distintos (cargar hilos, crear hilo, enviar mensaje, optimización de UI) sin que el hook estuviera declarado:

```tsx
// ❌ FALTABA esta línea — threads/setThreads no estaban declarados
const [threads, setThreads] = useState<ThreadSummary[]>([]);

// ...pero se usaban en 4 lugares:
loadThreads(...)  →  setThreads(data);                    // ReferenceError aquí
_createThread()   →  setThreads((c) => [thread, ...c]);
sendMessage()     →  setThreads((c) => [thread, ...c]);
sendMessage()     →  setThreads((c) => { ... });
```

Cuando React intentaba ejecutar `loadThreads`, el motor lanzaba:

```
ReferenceError: setThreads is not defined
```

En React 19 con boundaries de error inexistentes para este subtree, un `ReferenceError` no capturado durante un efecto **desmonta todo el árbol** → la ventana WebView2 queda con el `<div id="root">` vacío → fondo negro.

**El fix fue una sola línea** (commit `8fc71bb7`):

```tsx
const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
const [threads, setThreads] = useState<ThreadSummary[]>([]);  // ← añadido
const [messages, setMessages] = useState<PanelMessage[]>([]);
```

Adicionalmente se restauró el `useMemo` de `_activeThread` que referenciaba `threads`, para que el import de `useMemo` no quedara sin usar y se preservara la lógica original.

#### Lección

Un `useState` omitido no es un error de tipo en TypeScript (los setters se asumen del closure), ni falla en el bundle de Vite (esbuild no hace análisis de alcance). Solo explota **en runtime**, y solo en el camino feliz de un usuario autenticado — por eso pasó desapercibido. Esto argumenta a favor de un Error Boundary en el raíz de la app y/o un test E2E que cubra el flujo post-login.

### 2. Redacción de secrets en `Debug` (commit `8e5f9fa2`)

El test `security_hardening_test::test_security_hardening_debug_redaction` fallaba porque:

- `User` (`src/security/auth.rs`) tenía un `impl Debug` manual que **omitía** `api_key` por completo, pero el test esperaba ver `<redacted>`.
- `LoginRequest` (`src/auth2/mod.rs`) derivaba `Debug` automáticamente, lo que **filtraba** el `password` y `totp_code` en cualquier log de trazas.

```rust
// ANTES — LoginRequest derivaba Debug y filtraba credenciales
#[derive(Deserialize, Debug)]   // ❌ password en claro
pub struct LoginRequest { ... }

// DESPUÉS — Debug manual que redacta
impl fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .field("totp_code", &self.totp_code.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}
```

El test mismo tenía un bug latente: usaba `User::new()` que setea `api_key = ""`, y `"".contains("")` es siempre `true` en Rust, así que el assert de no-filtración era trivialmente cierto. Se corrigió para construir un `User` con una API key real.

### 3. El `pre-push` hook y la deuda técnica del workspace (commit `7a046656`)

El hook `.githooks/pre-push` usaba `#!/usr/bin/env pwsh`. PowerShell 7 (Core) **no está instalado** en el entorno de desarrollo (solo Windows PowerShell 5.1), así que todo push requería `--no-verify`. Se reescribió en `/bin/sh` (mismo patrón que el `pre-commit` que sí funcionaba), prefiriendo `pwsh` si existe y cayendo a `powershell`:

```sh
if command -v pwsh >/dev/null 2>&1; then
    PS_BIN="pwsh"
elif command -v powershell >/dev/null 2>&1; then
    PS_BIN="powershell"
fi
"$PS_BIN" -NoProfile -ExecutionPolicy Bypass -File "$REPO_ROOT/ci-local.ps1" -fast
```

Pero al validar el hook, `ci-local.ps1 -fast` **siempre fallaba**. Tres bugs encadenados:

1. **`$ErrorActionPreference = "Stop"`** trataba el progreso de `cargo` en stderr como error terminante y abortaba el script. Se reemplazó con un helper `Invoke-Native` que respeta `$LASTEXITCODE`.
2. **`cargo fmt --all`** y **`cargo check --workspace`** incluían `xavier-core`, un crate con **56 errores de compilación** (7 módulos declarados sin archivo). Se excluyó.
3. **`codegraph-parse-typescript`** tampoco compilaba (10 errores: API de `codegraph-types` refactorizada). Se excluyó también.

Tras el fix, el hook pasó limpio y el push a `main` se completó **sin `--no-verify`** por primera vez.

---

## Deuda Técnica Descubierta

El proceso de reparar el hook destapó tres ítems estructurales, ahora trackeados:

| Issue | Hallazgo | Impacto |
|-------|----------|---------|
| [#476](https://github.com/iberi22/xavier/issues/476) | `xavier-core` no compila (56 errores, 7 módulos faltantes) | Bloquea `cargo {fmt,check,clippy,test} --workspace` |
| [#477](https://github.com/iberi22/xavier/issues/477) | `codegraph-parse-typescript` stale tras refactor de `codegraph-types` | Mismo bloqueo de workspace |
| [#478](https://github.com/iberi22/xavier/issues/478) | 33 vulnerabilidades Dependabot (11 high) en `main` | Riesgo de seguridad en dependencias |

Ambos crates rotos (`#476`, `#477`) **no son dependencia de ningún otro crate** del workspace — son código incompleto que quedó en el `Cargo.toml` de miembros. Mientras no se resuelvan, el CI local (`ci-local.ps1`) los excluye explícitamente con `--exclude`.

---

## Alternatives & Trade-offs

**Para la pantalla negra**, la alternativa era añadir un Error Boundary. Se descartó como fix primario porque oculta el bug en lugar de resolverlo — el `ReferenceError` indica un estado mal cableado, no una excepción esperable. El Error Boundary queda como mejora futura de DX.

**Para los crates rotos**, la opción de "completarlos" se evaluó pero `xavier-core` tiene dependencias circulares con el crate principal (sus módulos importan `crate::agents::runtime::...` que no existen en `xavier-core`), así que copiar los archivos no funciona sin un refactor mayor. La exclusión del CI es el workaround seguro mientras se decide entre completar o archivar.

**Para el hook**, instalar `pwsh` (`winget install Microsoft.PowerShell`) era una alternativa válida, pero reescribir en `sh` es más portable: no asume nada sobre el entorno del contribuidor y unifica el patrón con `pre-commit`.

---

## Estado Final

- ✅ Pantalla negra resuelta — app de Windows instalada y verificada corriendo (`xavier-panel.exe` + sidecar en 8006, logs muestran peticiones a `/panel/api/*`).
- ✅ PR [#475](https://github.com/iberi22/xavier/pull/475) merged en `main`.
- ✅ `pre-push` hook funcional — push sin `--no-verify` confirmado.
- ✅ `ci-local.ps1 -fast` pasa limpio (`<<<< ALL CHECKS PASSED >>>>`).
- ⏳ Deuda técnica (#476, #477, #478) documentada para sesiones futuras.
- 📊 Suite de integración: **134/135** (1 fallo preexistente en `hierarchical_curation`, ortogonal).
