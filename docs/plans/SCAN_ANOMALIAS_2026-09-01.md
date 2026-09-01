# SCAN ANOMALÍAS Xavier v0.1.0 — 2026-09-01 15:38 UTC-5

> Escaneo profundo obligatorio pre-WAVE-7. Commit base: d3bf6683 (main). Validaciones: pnpm build PASS 3651→718ms, vitest 27/27 142/142, cargo fmt FAIL, cargo test (nix-shell) 2009 PASS, health degraded/unhealthy, release v0.1.0 FAILURE (33552143969), npm publish BLOCKED.

## Resumen ejecutivo

| Severidad | # | Componente |
|-----------|---|------------|
| 🔴 P0 Bloqueante release | A1 | `.cargo/config.toml` target-dir absoluto `/build/rust-target` rompe CI (3/3 builds failed) |
| 🔴 P0 Bloqueante docker | A2 | `docker-compose.dev.yml` XAVIER_PANEL_UI_DIR=/app/panel-ui/dist diverge de `assets.rs`+vite outDir `build` |
| 🟡 P1 Infra | A3 | `cargo fmt --check` FAIL (ci_version_gate.test.rs) |
| 🟡 P1 Supply | A4 | `@swal/preflight` no publicado en npm (404) → CI fallback pero `npx --yes` falla |
| 🟡 P1 Docker | A5 | Dockerfile frontend-builder usa `npm install` con `pnpm-lock.yaml` real |
| 🟢 P2 i18n | A6 | 3 archivos ES residual en `docs/SRC/` + 1 línea en DOCUMENTATION_MIGRATION |
| 🟢 P2 Config | A7 | Duplicado `overrides` panel-ui vs pnpm-workspace.yaml (redundante) |
| 🟢 P2 Vite | A8 | `vite.config.ts` proxy `/maloca` duplicado |
| 🟢 P2 Build | A9 | vite outDir `build` + postbuild `cp build→dist` — doble artefacto sin canon |
| 🔵 Info | A10 | DB unhealthy integrity_ok false wal 44MB (runtime, no código) |
| 🔵 Info | A11 | `docker-compose.yml` label `xavier.version` hardcode 0.4.1 vs 0.1.0 |
| 🔵 Info | A12 | `NPM_TOKEN` no configurado (`~/.npmrc` sin auth) |
| 🔵 Info | A13 | Windows release packaging path asume `target/` pero target-dir absoluto lo mueve |
| 🔵 Info | A14 | `release.yml` sin `CARGO_TARGET_DIR` env override (depende de config) |

---

## 1. i18n residual

```bash
grep -rn "Bienvenido\|Guía\|Instalación\|Requisitos\|Configuración" docs --include="*.md" | grep -v archive
```

Resultado (15 líneas): 11 en `docs/plans/PLAN_WAVE6...` (histórico, ignorar) + 4 productivas:

- `docs/SRC/REQUIREMENTS.md:1` # Requisitos Funcionales
- `docs/SRC/NON-FUNCTIONAL.md:1` # Requisitos No Funcionales
- `docs/SRC/index.md:12-13` links a REQUIREMENTS/NON-FUNCTIONAL
- `docs/explanation/DOCUMENTATION_MIGRATION.md:102` | **Es configuración inicial** | `setup/` | "Guía de instalación"

Acción: traducir `docs/SRC/*` a EN o mantener ES con `*.es.md` duplicado. `DOCUMENTATION_MIGRATION` línea 102 es tabla histórica, opcional.

## 2. Browser guards — OK

```bash
grep -rn 'from "@tauri-apps' panel-ui/src --include=".ts" --include=".tsx" | wc -l  # 0 OK
grep -rn "__TAURI_INTERNALS__" panel-ui/src --include=".ts" --include=".tsx" | wc -l  # 19 OK (>=7)
grep -rn "get_xavier_token" panel-ui/src | wc -l  # 0 OK
```

- `0` imports estáticos Tauri (7 dynamic `await import` en runtime Tauri branch) — PASS.
- `19` guards `__TAURI_INTERNALS__ in window` distribuidos: TopStatusBar, NotificationCenter, NotificationsDropdown, App, InputArea, ConfigModal, MalocaView, SystemScanStep, OnboardingFlow, UsageMetricsPanel, api/client — PASS.
- `useApiToken` hook 44L existe, `VITE_XAVIER_API_TOKEN` fallback — PASS.

## 3. Version sync — OK (periferia bloqueada)

```bash
bash scripts/check-version-sync.sh  # sync ok 0.1.0
cargo.toml version = "0.1.0"
panel-ui/vite.config.ts fs.readFileSync Cargo.toml x2 → __APP_VERSION__ OK
```

- `swal-preflight check --cwd .` vía `npx --yes @swal/preflight` → 404 Not Found (no publicado). Fallback local `node periferia/swal-preflight/bin/swal-preflight.js check` sí pasa (ci.yml lo usa). No bloquea local.

## 4. Health

```json
{
  "status": "unhealthy",
  "mode": "local-healthy",
  "database": {"status":"unhealthy","integrity_ok":false,"wal_size_bytes":44763832,"page_count":153136},
  "embedding": {"status":"healthy","model":"nomic-embed-text","hit_rate":0.9997},
  "llm": {"status":"healthy","provider":"local","reachable":true},
  "mesh": {"status":"degraded","active_peers":1,"libp2p_percent":10,"sync_lag_secs":233615},
  "system": {"cpu_usage":46.8,"ram_usage_percent":...}
}
```

- DB `integrity_ok false` + WAL 44MB → `unhealthy`. No es código: WAL sin checkpoint, `PRAGMA integrity_check` falla. Workaround doc: `XAVIER_MEMORY_SQLITE_PATH` backup + `VACUUM` o reinicio. No bloquea release pero debe entrar en KNOWN_ISSUES (ya está).
- Mesh degraded libp2p 10% esperado (solo local) — documentado, no bloquea.

`curl -H X-Xavier-Token` → 200 con 2 notifs system error/warning — OK.

## 5. Builds — PASS con deuda

```bash
pnpm --filter xavier-panel-ui run build  # ✓ 718ms, build/index.html 0.38kB, index.js 1173kB
  postbuild: node -e "fs.cpSync('build','dist')"  # cp build→dist manual
pnpm --filter xavier-panel-ui exec vitest run  # 27/27 142/142 49s (fileParallelism false + 30s)
ls panel-ui/build/index.html panel-ui/dist/index.html  # ambos 381B (idénticos)
```

Deuda: `vite outDir: "build"` diverge de convención Vite `dist`. Postbuild mantiene ambos sinc, pero Dockerfile y compose lo reflejan inconsistente.

## 6. Panel dist vs build — DIVERGENCIA P0

```bash
grep -rn "PANEL_BUILD_DIR\|panel-ui/build" src/server/panel/assets.rs Dockerfile panel-ui/vite.config.ts
```

- `panel-ui/vite.config.ts:99` outDir `build`
- `src/server/panel/assets.rs:9` PANEL_BUILD_DIR `panel-ui/build` (5 candidatos, priority XAVIER_PANEL_UI_DIR → exe_dir/build → cwd/build → CARGO_MANIFEST_DIR/build) — consistente con vite.
- `Dockerfile:79` COPY --from=frontend-builder /app/panel-ui/build /app/panel-ui/build — consistente con vite.
- `docker-compose.dev.yml:12` XAVIER_PANEL_UI_DIR=/app/panel-ui/dist — **DIVERGE** (espera dist, pero vite produce build, y cargo run sirve vía assets.rs que busca build; con volumen .:/app montado, /app/panel-ui/dist solo existe por cp postbuild, pero es copia no fuente).
- `vite.config.ts` postbuild hace `cp build→dist` para mantener compat, pero es workaround no canon.

Decisión propuesta (PLAN 6.04): alinear TODO a `build` (ya es lo que vite+assets.rs+Dockerfile usan) y cambiar `docker-compose.dev.yml` a `/app/panel-ui/build`. Alternativa alinear a `dist` requiere cambiar vite+assets.rs+Dockerfile+compose → más churn. `build` ya ganó.

## 7. Release v0.1.0 — FAILURE análisis

`gh run list --workflow=release.yml --limit 5` → 33552143969 `failure` (v0.1.0 push)

Logs `--log-failed`:

- `Build (x86_64-unknown-linux-gnu)` error: `failed to create directory /build/rust-target: Permission denied (os error 13)`
- `Build (aarch64-apple-darwin)` error: `failed to create directory /build/rust-target: Read-only file system (os error 30)`
- `Build (x86_64-pc-windows-msvc)` error: `The path target/.../xavier.exe either does not exist` (mismo root cause + Packaging step no encontró binario porque cargo lo puso en /build)

Root cause: `.cargo/config.toml` → `[build] target-dir = "/build/rust-target/xavier"` absoluto.

- En CI runners (ubuntu/macos), `/build` no existe y es read-only o sin permisos.
- Windows runner también falla porque target no está en `target/` donde `Compress-Archive` lo busca.
- `Swatinem/rust-cache@v2` cachea según target-dir pero falla antes de cache.

Fix: hacer target-dir relativo o condicional. Opción recomendada:

```toml
[build]
# target-dir = "target"  # default; overridden by CARGO_TARGET_DIR env in CI if needed
# O usar variable de entorno en config (no soportado directamente) → eliminar línea y dejar default,
# y en NixOS local usar CARGO_TARGET_DIR=/build/rust-target via env o direnv.
```

Alternativa minimal: cambiar a `target = "target"` relativo y documentar que NixOS devs usen `export CARGO_TARGET_DIR=/build/rust-target` en shell. Precedente: `docs/SWAL/VERSIONING.md` ya menciona `CARGO_TARGET_DIR=target` como workaround CI.

Release workflow también debe: (a) no depender de config absoluto, (b) empaquetar desde `$CARGO_TARGET_DIR` si está seteado, (c) Windows `cargo build --target` necesita target instalado (rust-toolchain con targets OK).

Adicional: `release.yml` publish-release y docker-publish dependen de `build-binaries` (needs) → nunca corrieron porque builds fallaron.

## 8. Docs links — OK

```python
bad = [l for l in re.findall(r'\[.+?\]\(.*?\)', README) if not l.startswith('http') and not Path(l.split('#')[0]).exists()]
→ []  # 0 rotos
```

README 16 links: 5 shields + `docs/guides/WINDOWS_INSTALL.md` + releases + `docs/site/src/content/docs/downloads.mdx` + `docs/KNOWN_ISSUES.md` + AGENTS.md + agent-integration + FEATURE_STATUS + CLI_REFERENCE + MCP_INTEGRATION + QUICKSTART + ARCHITECTURE + LICENSE — todos PASS.

## 9. cargo + secrets — FAIL fmt

```bash
cargo fmt --check  # FAIL
Diff in tests/ci_version_gate.test.rs:17,48,61,96 (fs::write + assert! formatting)
rg -n "sk-or|ghp_|Bearer" --hidden  # solo ejemplos/masks (ModelSelector.svelte sk-or-a...52f9 es mask demo), .env.example Bearer coment, MANUAL.md Authorization Bearer *** — OK, no leak
```

Fix: `cargo fmt` (automático).

`.env` no commiteado, `XAVIER_TOKEN` en env ok.

## 10. NPM publish — BLOQUEADO (Prioridad 2)

```bash
npm whoami  # ENEEDAUTH
cat ~/.npmrc  # sin _authToken, solo prefix/ignore-scripts
~/proyectosSWAL/periferia/swal-preflight/package.json  # name @swal/preflight 0.1.0 bin swal-preflight.js
npm pack --dry-run (en ~/apps/xavier) → xavier-monorepo-0.1.0.tgz 6.2MB (2189 files) — OJO: corre en xavier, no en periferia (confusión)
~/periferia/swal-preflight npm pack --dry-run → debe dar @swal/preflight 0.1.0 10.8kB (no ejecutado aquí)
```

`iberi22/swal-preflight` repo creado 3 commits, npm publish --access public pendiente. Requiere `NPM_TOKEN` env o `npm login`. Sin token no se puede publicar (no inventar).

CI `ci.yml:135` usa `npx --yes @swal/preflight check --cwd . || swal-preflight check --cwd . || bash scripts/check-version-sync.sh` con fallback — funciona sin npm.

## 11. Docker — hallazgos

- `Dockerfile` Stage 0 `frontend-builder`: `COPY panel-ui/package.json panel-ui/package-lock.json*` + `RUN npm install` → pero repo usa `pnpm` + `pnpm-lock.yaml` + `pnpm-workspace.yaml`. `npm install` genera `package-lock.json` efímero y no respeta `overrides` de pnpm. Debe ser `pnpm install`.
- `Dockerfile` no copia `pnpm-workspace.yaml`, `pnpm-lock.yaml` → build no reproducible.
- `docker-compose.yml` `image: xavier:${XAVIER_IMAGE_TAG:-0.0.1}` default 0.0.1 ok, pero `labels: xavier.version=0.4.1` hardcode stale.
- `docker compose -f docker-compose.yml -f docker-compose.dev.yml config | grep XAVIER_DEV_MODE` → `XAVIER_DEV_MODE: "true"` OK, pero `XAVIER_TOKEN: dummy-dev-token` + `command: cargo run -- http --mcp-port 0` requiere toolchain Rust en container (no está en debian:bookworm-slim runtime; dev override asume volumen .:/app con cargo local — funciona solo si HOST tiene Rust, no en container).
- `cargo fmt` no afecta docker, pero `assets.rs` priority list no incluye `/app/panel-ui/dist` (solo build) → dev compose con dist fallaría si dist no existe.

## 12. Otros

- `pnpm -w run lint` → biome 4 files OK, tsc generative OK.
- `pnpm build` postbuild `cp build→dist` asegura ambos existem pero es tech debt.
- `panel-ui/package.json` `overrides: {sharp}` + `pnpm-workspace.yaml overrides: {undici, esbuild, sharp}` duplicado sharp — no bloquea pero redundante.
- `vite.config.ts` proxy `/maloca` duplicado (líneas 48 y 62) — harmless pero lint.
- `swal-preflight` skill registry `~/.hermes/skill-registry.json` no existe — no afecta xavier pero task 6 pide añadir entry.

---

## Propuesta WAVE-7 (issues feat- canónicos, skill xavier-jules-wave 11 secciones)

> Cada issue usa template canónico: Title `feat(scope): ...`, Description (Context/Problem/Solution), 4 web research queries, Acceptance Criteria con `cargo check`/`grep`/`pnpm build`, Files to modify, Testing, Rollback.

### WAVE-7.01 `fix(ci): make cargo target-dir portable (remove absolute /build)`

**Context:** release 33552143969 falló 3/3 por `/build/rust-target` no escribible en GH Actions. **Solution:** quitar `target-dir = "/build/..."` de `.cargo/config.toml`, dejar default `target/`, documentar `CARGO_TARGET_DIR=/build/rust-target` para NixOS devs vía `direnv`/`shell.nix`. Actualizar `release.yml` para usar `$CARGO_TARGET_DIR` fallback y empaquetar desde ahí.

- Files: `.cargo/config.toml`, `.github/workflows/release.yml`, `docs/guides/LOCAL_SETUP.md` (nota NixOS), `shell.nix`/`flake.nix` si aplica
- AC: `grep -c "/build/rust-target" .cargo/config.toml ==0`, `gh workflow run release.yml --ref main` → 3 builds PASS (o al menos linux PASS), `cargo build --release --target x86_64-unknown-linux-gnu --features ci-safe` en CI sin error mkdir

### WAVE-7.02 `fix(panel): unify vite outDir and XAVIER_PANEL_UI_DIR to build (remove cp workaround)`

**Context:** vite `build` vs `dist` + compose.dev `dist` vs assets `build`. **Solution:** alinear todo a `build` (canon actual). Cambiar `docker-compose.dev.yml` XAVIER_PANEL_UI_DIR a `/app/panel-ui/build`, eliminar postbuild `cp build→dist` o mantenerlo como compat temporal con comentario, actualizar docs.

- Files: `docker-compose.dev.yml`, `panel-ui/package.json` (remove postbuild cp o dejar con comment), `docs/KNOWN_ISSUES.md` (update A11 resolution)
- AC: `grep -rn PANEL_BUILD_DIR src/server/panel/assets.rs | grep -q build`, `grep -q "panel-ui/build" docker-compose.dev.yml`, `pnpm build && ls panel-ui/build/index.html && docker compose -f docker-compose.yml -f docker-compose.dev.yml config | grep -q build`

### WAVE-7.03 `fix(docker): use pnpm in frontend-builder and sync lockfiles`

**Context:** Dockerfile usa npm en repo pnpm → no reproducible. **Solution:** `FROM node:20` + `RUN corepack enable && corepack prepare pnpm@latest --activate`, `COPY pnpm-lock.yaml pnpm-workspace.yaml`, `RUN pnpm install --frozen-lockfile`, `RUN pnpm --filter xavier-panel-ui run build`, fix label version 0.0.1.

- Files: `Dockerfile`, `docker-compose.yml` (label fix 0.0.1)
- AC: `grep -q "pnpm install" Dockerfile`, `grep -q '0.0.1' Dockerfile` (label), `docker build -f Dockerfile --target frontend-builder` (smoke)

### WAVE-7.04 `chore(fmt): cargo fmt and biome fixes`

**Context:** `cargo fmt --check` FAIL. **Solution:** `cargo fmt`, fix vite duplicate proxy, dedupe overrides.

- Files: `tests/ci_version_gate.test.rs` (fmt), `panel-ui/vite.config.ts` (dedupe maloca), `panel-ui/package.json` vs `pnpm-workspace.yaml` (dedupe sharp note)
- AC: `cargo fmt --check` PASS, `pnpm -w run lint` PASS, `grep -c "/maloca" panel-ui/vite.config.ts ==1`

### WAVE-7.05 `chore(i18n): translate docs/SRC remaining ES to EN`

**Context:** 4 líneas ES residual no críticas. **Solution:** traducir `docs/SRC/REQUIREMENTS.md`, `NON-FUNCTIONAL.md`, `index.md` encabezados a EN, fix DOCUMENTATION_MIGRATION línea 102.

- Files: `docs/SRC/*.md`, `docs/explanation/DOCUMENTATION_MIGRATION.md`
- AC: `grep -rn "Requisitos" docs/SRC --include="*.md" | wc -l ==0`, `grep -rn "Bienvenido" docs --include="*.md" | grep -v archive | wc -l ==0` (true residual 0)

### WAVE-7.06 `feat(docs): skill registry and npx preflight reproducibility`

**Context:** `~/.hermes/skill-registry.json` no existe, README no menciona `npx @swal/preflight`. **Solution:** añadir entry skill registry (si aplica) y documentar en README + CONTRIBUTING `npx --yes @swal/preflight check` fallback.

- Files: `~/.hermes/skill-registry.json` (global), `README.md`, `CONTRIBUTING.md` o `AGENTS.md`
- AC: `cat ~/.hermes/skill-registry.json | jq -e '.skills[] | select(.name=="swal-preflight")'`, `grep -q "npx.*preflight" README.md`

### WAVE-7.07 `fix(release): windows packaging use CARGO_TARGET_DIR and verify archive`

**Context:** Windows `Compress-Archive` falla cuando target-dir absoluto mueve binario. **Solution:** en release.yml, resolver `BIN_PATH="${CARGO_TARGET_DIR:-target}/${{ matrix.target }}/release/${{ matrix.binary_name }}"` y validar existencia antes de comprimir.

- Files: `.github/workflows/release.yml`
- AC: `grep -q "CARGO_TARGET_DIR" .github/workflows/release.yml`, `gh run view <id> --log` sin "does not exist"

### WAVE-7.08 `docs(known-issues): update DB wal and mesh degraded notes with remediation`

**Context:** health wal 44MB + integrity false + mesh degraded ya parcialmente doc pero sin remediation steps. **Solution:** actualizar `docs/KNOWN_ISSUES.md` con `VACUUM`, `PRAGMA wal_checkpoint(TRUNCATE)`, `XAVIER_SYNC_*` tuning.

- Files: `docs/KNOWN_ISSUES.md`
- AC: `grep -q "wal_checkpoint" docs/KNOWN_ISSUES.md`

**Estimación WAVE-7:** 8 issues, 260k tokens, 65 turns, single session muse-spark. Orden: 7.01→7.07 (bloqueante release) primero, luego 7.02/7.03, 7.04, resto docs.

---

## Checklist verificación final (para cierre)

```bash
grep -rn 'from "@tauri-apps' panel-ui/src --include=".ts" --include=".tsx" | wc -l # 0
grep -rn "__TAURI_INTERNALS__" panel-ui/src --include=".ts" --include=".tsx" | wc -l # >=7
grep -rn "get_xavier_token" panel-ui/src | wc -l # 0
bash scripts/check-version-sync.sh # sync ok 0.1.0
cargo fmt --check # PASS
pnpm --filter xavier-panel-ui run build 2>&1 | tail -5 # PASS
pnpm --filter xavier-panel-ui exec vitest run 2>&1 | grep -E "Test Files|Tests" # 27/27
ls panel-ui/build/index.html && ls panel-ui/dist/index.html # both exist
grep -q "/build/rust-target" .cargo/config.toml && echo FAIL || echo PASS
docker compose -f docker-compose.yml -f docker-compose.dev.yml config | grep -q XAVIER_DEV_MODE
curl -s http://127.0.0.1:8006/health | python3 -m json.tool | head -20
curl -s -H "X-Xavier-Token: $XAVIER_TOKEN" http://127.0.0.1:8006/notifications | head -c 200
```

*Cierre: SCAN completo. Siguiente paso: ejecutar fixes P0 (7.01, 7.02, 7.03, 7.04) sin wave, luego dispatch WAVE-7 restante si hace falta.*
