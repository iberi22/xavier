# M5 — Plan por fases: auditoría adversaria + cierre de defects

Repo: `/home/belal/proyectosSWAL/apps/xavier` (rama `main`, HEAD `5718de47`)

## Estado heredado de M4 (VERIFICADO, no trustworthy de palabra)

- Commit `5718de47 feat(ledger): register Wave-31 features in features.json (WAVE-31.01-04)`
- `.gitcore/features.json`: 96 features, 90 con `passes:true` (93.8% del ledger),
  metadata declara `overall_progress_pct: 92.3`. La cifra que segwrue swore
  (92.3%) viene de `metadata`, NO de recalcular `passes`.
- Issues #2805 #2806 #2808 #2809 #2810 #2811: los 6 CLOSED (verificado vía `gh issue view`).
- `cargo test --package xavier --lib --features ci-safe`: **3016 passed, 0 failed, 1 ignored (211.91s)**.

## Residuo sin trackear (deja el enjambre de verificación)

- `.agents/teamwork/` — 15 carpetas de agentes, informes parciales
- `docs/REVISION-EXTERNA.md` (22 KB) — revisión adversarial del 2026-10-01, veredicto
  "la fase no se puede cerrar" con 3 bloqueantes
- `tests/adversarial_clavis_challenge.rs` y `tests/adversarial_challenge_2_stress.rs`
  — ~10 KB cada uno, escritos por los challengers, NUNCA trackeados ni ejecutados por CI
- `CLAUDE.md` (4.9 KB)

## Defectos de REVISION-EXTERNA.md (re-verificados por mí contra el código actual)

- **D1 ALTA** — `validate_key_name` (clavis.rs:329) NO restringe a `api_key_`; acepta
  `DB_MASTER_KEY`, `JWT_PRIVATE_KEY`, `node_secret_*`. El vault de respaldo es un
  `OnceLock` global (`vault.rs:24`) con dir único `~/.xavier/secrets/` y clave derivada
  SIN el service_name (vault.rs:51-56). `get_secret` cae al fallback en cualquier error,
  `NoEntry` incluido. Confirmado leyendo código actual.
- **D2 MEDIA** — `/v1/clavis` NO está en `ROOT_ONLY_PREFIXES` (`src/cli/http_setup.rs:40`,
  solo `/secrets/`, `/security/tokens`, `/v1/security/`, `/v1/proxy/request`). El router
  productivo es `src/cli/server.rs:1044`, los tests usan `create_router()` distinto.
- **D3 ALTA** — `prune_precondition`/`prune_block_reason`/`record_is_observable` solo se
  llaman desde tests; `TgdUtilityPruner::prune_memories` borra sin consultarlos.
- **D4 ALTA** — `maybe_flush` (access.rs:370) sin temporizador ni flush al apagar.
- **D5/D6/D7/D8 MEDIA** — doble conteo en flush fallido; se registran accesos a memorias
  filtradas por clearance; cursores de ingesta que pierden datos o se envenenan con fechas futuras.
- **D9 MEDIA** — `.husky/pre-commit` perdió `check-strategy-leak.sh`, el bloqueo
  `*.db|*wal|*shm` y `check-secrets.sh` obligatorio. `.husky/pre-push` usa `@{u}` (rama sin
  upstream escanea TODO el historial) y detecta `--force` leyendo `$*` (código muerto).
- **D10 MEDIA** — arranque frágil: `merge_runtime_state` falla → `main.rs:64` aborta el proceso.

## Fases (2 subagentes Hermes por fase)

| Fase | Alcance | Agente A | Agente B | Gate |
|---|---|---|---|---|
| **F1** | Gate oficial + residuo | `verify-pipeline.sh` completo + clasificación del residuo untracked |，健康 de los tests adversariales: ¿compilan? ¿pasan? | pipeline exit 0 |
| **F2** | D1+D2 (seguridad) | Reproducir D1 empíricamente + test que lo fije | Implementar fix del allowlist `api_key_` + `ROOT_ONLY_PREFIXES` | test rojo→verde |
| **F3** | D3+D4+D5+D6 (prune) | Conectar gate de prune al pruner | Flush al apagar + arreglar doble conteo | gate llamado |
| **F4** | D7+D8+D9+D10 | Cursores de ingesta (Hermes remember-tarde, OpenCode future-date) | Restaurar guardas de hooks + arranque resiliente | hooks verdes |
| **F5** | Auditoría externa + cierre | Claude Code CLI (alias `fable`) revisión adversaria tier high | Consolidar informe final + actualizar ledger | informe con evidencia |

## Reglas de ejecución

- Cada subagente es self-contained: objetivo, rutas exactas, criterios de verificación.
- Ningún subagente hace `git add`/`git commit`/`git push`. El orquestador revisa y commit-ea.
- `export PKG_CONFIG_PATH=$HOME/.nix-profile/lib/pkgconfig` en cualquier cargo.
- Prohibido `rm -rf target`. Cache: skill `rust-build-cache`.
- Nada se marca "hecho" sin comando pasado con output (REGLA #7).
