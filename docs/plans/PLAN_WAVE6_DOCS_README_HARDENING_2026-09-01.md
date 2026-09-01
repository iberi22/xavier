# PLAN WAVE-6 — Docs / README / Docker / Releases hardening (analysis mientras WAVE-5 corre)

> Fecha: 2026-09-01 — análisis en paralelo a WAVE-5 (browser-compat 1771-1780)
> Estado: ANÁLISIS — no tocar código hasta que WAVE-5 mergee; preparar wave siguiente
> Objetivo: dejar Xavier 100% sólido para distribución con docs en inglés, docker dev + UI, descargas claras y bugs conocidos documentados

## 1. Dictamen README + links

**README actual:** `README.md` 261 líneas, inglés 95%, 8 badges/links verificados:
- `docs/guides/WINDOWS_INSTALL.md` OK
- `AGENTS.md` OK
- `docs/guides/agent-integration.md` OK
- `docs/FEATURE_STATUS.md` OK (pero dice `1.0.0 52/52 100%` — desalineado con versioning `0.0.1` canon)
- `docs/guides/CLI_REFERENCE.md` OK
- `docs/guides/MCP_INTEGRATION.md` OK
- `docs/guides/QUICKSTART.md` OK
- `docs/ARCHITECTURE.md` OK
- `LICENSE` OK

**Gaps README (falta para distribución):**
- No sección `📦 Downloads / Releases` (binarios multi-platform, checksums, `gh release` link)
- No sección `🐳 Docker` (quickstart `docker compose up`, dev vs prod)
- No sección `🖥️ UI Modes` (browser `http://127.0.0.1:8006/` vs Tauri `pnpm tauri dev` vs `XAVIER_PANEL_UI_DIR`)
- No sección `🐛 Known Issues / Broken Config Paths`
- No badge/link a `docs/site` (Starlight) ni a GitHub Releases
- `FEATURE_STATUS` dice `1.0.0` cuando canon es `0.0.1` → drift

## 2. Idioma — 15 archivos en español que DEBEN ser inglés

| Archivo | Hits | Acción |
|---------|------|--------|
| `docs/api/README.md` | Bienvenido, Configuración | Reescribir a inglés, mover ES a `docs/api/README.es.md` opcional |
| `docs/LOCAL_SETUP.md` | Guía, Prerrequisitos | Inglés + `LOCAL_SETUP.es.md` |
| `docs/OPERATIONS.md` | Guía | Inglés — es runbook crítico |
| `docs/XAVIER_RAG_GUIDE.md` | Configuración, Guía | Inglés — guía RAG principal |
| `docs/guides/WINDOWS_INSTALL.md` | Configuración, Instalación, Requisitos | Inglés (o dual) |
| `docs/guides/CODEGRAPH_GIT_SYNC.md` | Instalación | Revisar |
| `docs/SRC/*.md` | Requisitos | Títulos en inglés, cuerpo puede quedar ES si es glosario pero preferible EN |
| `docs/archive/*` | varios | No bloquear — archivar como está, no migrar |
| `docs/skills/xavier-issue-creation/SKILL.md` | Idioma | Inglés |

**Política:** fuente primaria SIEMPRE inglés. Si se mantiene ES, duplicar como `*.es.md` y linkear desde EN.

## 3. Docker — falta modo dev y UI

**Actual:**
- `Dockerfile` multi-stage (frontend-builder node:20 → rust:1.90 builder → final ~500MB), build `cargo build --release --bin xavier -j 1 --features local-gllm,cli-interactive`, copia `panel-ui/src-tauri` pero NO documenta dev.
- `docker-compose.yml` servicio `xavier` image `xavier:0.0.1`, env `XAVIER_TOKEN` requerido, `XAVIER_HOST=0.0.0.0`, `host.docker.internal` para Ollama, healthcheck `curl /health`, volumes `xavier_data:/data`, `xavier_logs:/logs`, mount `.:/mnt/workspaces/xavier:ro`.
- **Falta:** `XAVIER_DEV_MODE` no inyectado, no perfil dev, no `docker-compose.dev.yml`, no `Dockerfile.dev` con `cargo run` + hot-reload panel.
- **UI:** `src/server/panel/assets.rs` resuelve `panel-ui/build` vía `XAVIER_PANEL_UI_DIR` env → exe_dir → cwd → `CARGO_MANIFEST_DIR/panel-ui/build`. Pero `Dockerfile` hace `panel-ui/build` (no `dist`) — drift con `panel-ui/dist` real del build. `vite.config.ts` outDir debe alinearse.

**Propuesta:**
- `docker-compose.dev.yml` override: `XAVIER_DEV_MODE=true`, `XAVIER_TOKEN=dummy-dev-token`, `RUST_LOG=debug`, `XAVIER_PANEL_UI_DIR=/app/panel-ui/dist`, volume `.:/app` + `command: cargo run -- http --mcp-port 0`.
- Documentar en README `## 🐳 Docker` con tabla `mode | compose file | auth | panel`.
- Añadir `XAVIER_DEV_MODE` a `docker-compose.yml` como ` - XAVIER_DEV_MODE=${XAVIER_DEV_MODE:-false}`.

## 4. Descargables / Releases portal

**Actual:**
- `.github/workflows/release.yml` tag `v*` → matriz 3 targets (`x86_64-unknown-linux-gnu` ubuntu, `aarch64-apple-darwin` macos-14, `x86_64-pc-windows-msvc` windows), `cargo build --release --target … --features ci-safe`, empaqueta `xavier-v*.<target>.tar.gz/.zip` + `.sha256`, upload artifact 7d, `publish-release` descarga artifacts y crea GitHub Release con `softprops/action-gh-release`.
- **Falta:** no publica `ghcr.io/iberi22/xavier` docker, no badge `Releases` en README, no página `docs/site/src/content/docs/downloads.md` ni `docs/reference/DOWNLOADS.md`, no `RELEASES.md` changelog link.
- `deploy-docs.yml` publica `docs/site` (Starlight) a Pages, pero no incluye panel builds.

**Propuesta:**
- README nueva sección `## 📦 Downloads` con tabla OS/arch + `curl -L https://github.com/iberi22/xavier/releases/latest/download/...` + sha256 verify.
- `docs/site` nueva página `downloads.mdx` generada desde `release.yml` matrix.
- Opcional: job `docker-publish` en `release.yml` para `ghcr.io/iberi22/xavier:${tag}`.

## 5. Bugs conocidos y caminos rotos de configuración

**Recopilado 2026-09-01 (para documentar en `docs/KNOWN_ISSUES.md` + README `Known Issues`):**

| # | Bug / Camino roto | Impacto | Workaround | Fix en curso |
|---|-------------------|---------|------------|--------------|
| 1 | `panel-ui` sin guard Tauri → `invoke`/`listen` crash `TypeError/Cannot read transformCallback` | 🔴 Crítico browser | WAVE-5 1771-1777 | Sí |
| 2 | `get_xavier_token` → `""` → 401 loop `/v1/memories`/`/notifications` | 🔴 | WAVE-5.01 hook `VITE_XAVIER_API_TOKEN` | Sí |
| 3 | `GET /v1/config` 404, existe `/v1/config/providers` | 🟡 Medio `hasConfig` never true | WAVE-5.05 fallback true | Sí |
| 4 | `GET /panel/api/config` 404 | 🟡 | Usar `/v1/config/providers` | WAVE-5.05 |
| 5 | `AuthProvider` refresh 400 limpia `token` → pierde `API_TOKEN` | 🟡 logout involuntario | WAVE-5.06 preserva | Sí |
| 6 | `InputArea` `scan_project_folder` sin File API fallback | 🟡 botón muerto browser | WAVE-5.04 | Sí |
| 7 | `vite __APP_VERSION__` hardcode `0.6.1-beta` vs `0.0.1` | 🟢 | WAVE-5.08 leer Cargo | Sí |
| 8 | `pnpm.overrides` deprecated warning | 🟢 | WAVE-5.08 migrar | Sí |
| 9 | `XAVIER_TOKEN=foo # comment` inline → token incluye `# comment` | 🟡 auth fail silencioso | Poner comentario en línea propia | Doc fix |
| 10 | `.env` vs `config/xavier.config.json` canon — env var funnel pero no documentado | 🟡 confusión | Settings loader central | Doc fix |
| 11 | `panel-ui/build` vs `panel-ui/dist` drift (Dockerfile vs vite vs assets.rs) | 🟡 Docker sirve vacío | Alinear a `dist` | WAVE-5/6 |
| 12 | `XAVIER_WORKSPACE_DIR=.` default pero `panel_store` usa `workspace_id=default` | 🟢 | Doc clarificar | Doc fix |
| 13 | `FEATURE_STATUS.md` dice `1.0.0 52/52` cuando versión canon `0.0.1` | 🟢 drift | WAVE-6 fix | Pendiente |
| 14 | `docs/api/README.md` ES cuando resto README EN | 🟢 i18n | WAVE-6 | Pendiente |
| 15 | `docker-compose` sin `XAVIER_DEV_MODE` | 🟡 dev no arranca sin token | WAVE-6 compose.dev | Pendiente |

**Caminos rotos config:**
- `XAVIER_TOKEN` inline comments (ya en README note pero falta en `docs/reference/ENV_VARS.md`).
- `XAVIER_PANEL_UI_DIR` no documentado en README `Environment Variables` tabla.
- `XAVIER_CODE_GRAPH_DB_PATH`, `XAVIER_MEMORY_*_PATH` en compose pero no en README.
- `XAVIER_PANEL_UI_DIR` priority list (5 candidates) solo en `assets.rs` comentario, no en docs.

## 6. Propuesta WAVE-6 (post WAVE-5 merge)

**Precond:** WAVE-5 1771-1780 merged + `cargo check` verde + `pnpm build` verde

| # | Issue | Foco | Risk | E2E |
|---|-------|------|------|-----|
| 6.01 | `docs(xavier): translate 15 ES docs to EN, keep *.es.md` | i18n | LOW | `grep -r Bienvenido docs --include="*.md" | wc -l`==0, `ls docs/**/*.es.md`>=4 |
| 6.02 | `feat(docker): compose.dev.yml + XAVIER_DEV_MODE + UI dev mode` | docker | MED | `docker compose -f docker-compose.dev.yml config | grep DEV_MODE` + `curl localhost:8006/health` |
| 6.03 | `docs(readme): add Downloads + Docker + UI Modes + Known Issues` | readme | LOW | `grep -c "Downloads" README.md`>=1, todos links `OK` via script |
| 6.04 | `fix(panel): align panel-ui/dist vs build + Dockerfile + assets.rs` | panel | LOW | `pnpm build && ls panel-ui/dist/index.html` + `grep dist Dockerfile` |
| 6.05 | `docs(site): downloads page + releases badge` | site | LOW | `astro build` incluye `/downloads/` |
| 6.06 | `docs(known-issues): KNOWN_ISSUES.md + ENV_VARS fix inline comments` | docs | LOW | `grep -c "inline" docs/reference/ENV_VARS.md` |
| 6.07 | `chore(ci): release docker ghcr + docs drift gate` | ci | MED | `release.yml` has `ghcr` job |
| 6.08 | `chore(feature-status): align 0.0.1 + SRS REQ-045 i18n` | versioning | LOW | `grep 0.0.1 FEATURE_STATUS.md` |

**Estimación:** 8 issues, 280k tokens, 70 turns — single session muse-spark. Usar mismo protocolo `swal-preflight preflight --wave 8`.

## 7. Checklist pre-WAVE-6 (para orquestador)

- [ ] WAVE-5 merged, `git pull main`, `swal-preflight check` sync ok (0.0.1)
- [ ] `grep -r "Bienvenido" docs` baseline 15 files before translation
- [ ] `docker compose config` funciona sin `XAVIER_TOKEN` cuando `XAVIER_DEV_MODE=true`
- [ ] `panel-ui/dist/index.html` existe tras `pnpm build`
- [ ] `gh release view v0.0.1 --json assets --jq length` >=3 (o pendiente hasta tag `v0.0.1`)

## 8. Referencias

- `README.md` 261 lines
- `docs/api/README.md` ES 118 lines → debe EN
- `docs/LOCAL_SETUP.md` ES 191 lines
- `docs/OPERATIONS.md` ES 285 lines
- `Dockerfile` 1 stage frontend-builder + builder
- `docker-compose.yml` xavier:8006 + healthcheck
- `.github/workflows/release.yml` 3 targets, 7d artifacts, publish-release
- `src/server/panel/assets.rs` `PANEL_BUILD_DIR="panel-ui/build"` drift
- `FEATURE_STATUS.md` dice `1.0.0` drift

---
*Análisis hecho con `rg`, `cargo check`, `pnpm build`, `gh pr/issue` — listo para crear wave-6 cuando WAVE-5 cierre.*
