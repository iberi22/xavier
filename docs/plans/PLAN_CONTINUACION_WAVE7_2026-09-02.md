# PLAN Continuación Xavier 0.1.0 → 0.1.1 — Gaps y Siguientes Pasos

Fecha: 2026-09-02 00:00 UTC-5
Base: main 95c89b96 (4 commits sobre v0.1.0), SCAN 01 sep, WAVE-7 1791-1795 dispatched
Objetivo: cerrar gaps restantes sin re-auditar WAVE-5/6 ya verificados

## 1. Gaps cerrados (ya fixeados y verificados 01 sep)

| # | Gap | Fix | Commit | Verificado |
|---|-----|-----|--------|------------|
| A1 | .cargo/config.toml /build rompe CI | target-dir eliminado, CARGO_TARGET_DIR=target env | 94826cc3 | gh run 33560794902 3 builds success |
| A2 | compose.dev dist vs build | XAVIER_PANEL_UI_DIR /build | 94826cc3 | docker compose config ok |
| A3 | cargo fmt FAIL ci_version_gate | cargo fmt | 94826cc3 | cargo fmt --check 0 |
| A5 | Dockerfile npm vs pnpm | pnpm 9.12.3 frozen-lockfile | 94826cc3 | pnpm build 651ms |
| A8 | vite duplicate /maloca | removido | 94826cc3 | pnpm build ok |
| A11 | label 0.4.1 stale | 0.1.0 | 94826cc3 | grep ok |
| A13 | windows packaging BIN_PATH | CARGO_TARGET_DIR resolve | 94826cc3 | build windows success |
| A14 | sha256sum missing macOS | sha256sum || shasum -a 256 | 95c89b96 | darwin success |
| - | Cargo.lock 0.0.1->0.1.0 | sync | b93c7ff0 | check-version-sync 0.1.0 |
| - | periferia README ES | EN repo-only + alias | 4855a0d | git push ok |

Verificación repetida 01 sep 21:00-21:55:
  check-version-sync 0, cargo fmt 0, cargo check 0.85s, cargo test 2009/2009, pnpm build, vitest 27/27, guards 0/19/0, /panel 200

## 2. Gaps abiertos (mapeados a WAVE-7)

| Gap | Severidad | Issue | Isla | Estado |
|-----|-----------|-------|------|--------|
| G1 | P0 E2E | Playwright generative-ui.spec.ts drift OpenUI cockpit vs XAVIER LOGIN | 1791 panel-ui/tests/generative-ui.spec.ts | Jules in_progress |
| G2 | P2 i18n | docs/SRC 5 líneas ES (Requisitos) | 1792 docs/SRC/* | Jules in_progress |
| G3 | P1 infra | DB wal 55MB unhealthy, pragma checkpoint | 1793 src/storage/pragma.rs + src/server/health.rs | Jules in_progress |
| G4 | P2 docs | KNOWN_ISSUES sin WAL remediation | 1794 docs/KNOWN_ISSUES.md | Jules in_progress (disjoint de G3) |
| G5 | P2 docs | README/QUICKSTART aún npx vs repo | 1795 README.md + docs/guides/QUICKSTART.md | Jules in_progress |
| G6 | P1 release | Tag v0.1.0 en 17fa47c0 viejo, builds nuevos en 95c89b96 | - | Manual: tag v0.1.1 |
| G7 | P2 health | mesh degraded libp2p 10% (esperado local) | - | Doc, no bloquea |
| G8 | P2 build | pnpm build cp build->dist workaround (doble artefacto) | - | Opcional WAVE-7 post |

G6 no tiene issue (es operación git tag). G7/G8 son seguimiento, no wave.

## 3. Siguientes pasos (orden)

### Fase 1 — Esperar WAVE-7 (async Jules, 2-4h)
- `gh issue list --search "wave-7" --json number,state --limit 10` cada 30m
- Cuando Jules abra PRs 1791-1795, revisar `gh pr list --search "1791"` etc

### Fase 2 — Integrar PRs (manual, en orden 1..5)
Para cada PR (ej 1791):
  `gh pr view <num> --json files --jq '.files[].path'`
  `gh pr checkout <num> && cargo check -p xavier --all-targets && pnpm --filter xavier-panel-ui exec vitest run`
  Si verde: `gh pr merge <num> --squash --delete-branch`
  Si conflicto: `git pull --rebase origin main` + re-run verificación
Orden: 1791 (test) -> 1792 (docs) -> 1793 (storage) -> 1794 (docs) -> 1795 (docs) — islas disjoint, sin orden estricto pero 1793 antes que 1794 es preferible (code antes docs)

### Fase 3 — Tag release v0.1.1
```
git checkout main && git pull
git tag -a v0.1.1 -m "chore(release): 0.1.1 - WAVE-7 hardening (wal, i18n, e2e, preflight docs)"
git push origin v0.1.1
gh run list --workflow=release.yml --limit 3
gh release view v0.1.1 --json assets --jq '.assets[].name'
```
Verificar 3 artefactos: xavier-v0.1.1-x86_64-unknown-linux-gnu.tar.gz, aarch64-apple-darwin.tar.gz, x86_64-pc-windows-msvc.zip + .sha256

### Fase 4 — Verificación final post-merge
```
bash scripts/check-version-sync.sh  # expect 0.1.1 sync ok
cargo fmt --check
RUSTC_WRAPPER="" SCCACHE_DISABLE=1 nix-shell -p gcc glibc --run "cargo test --package xavier --lib --features ci-safe -- --test-threads=2 2>&1 | tail -5"  # 2009+ passed
pnpm --filter xavier-panel-ui run build  # 651ms
pnpm --filter xavier-panel-ui exec vitest run  # 27/27
PANEL_UI_BASE_URL=http://127.0.0.1:8006 pnpm --filter xavier-panel-ui exec playwright test tests/generative-ui.spec.ts --reporter=list  # 2 passed (tras 1791)
curl -s http://127.0.0.1:8006/health | python3 -m json.tool | head -20  # wal < 10MB tras 1793
curl -s -H "X-Xavier-Token: $XAVIER_TOKEN" http://127.0.0.1:8006/notifications | head -c 200  # 200
grep -rn "Requisitos" docs/SRC --include="*.md" | wc -l  # 0 tras 1792
grep -c "periferia/swal-preflight" README.md  # >=1 tras 1795
```

### Fase 5 — Cierre docs
- Actualizar `docs/FEATURE_STATUS.md` si cambia
- Actualizar `docs/plans/SCAN_ANOMALIAS_2026-09-01.md` con estado post-WAVE-7
- `node ~/proyectosSWAL/periferia/swal-preflight/bin/swal-preflight.js check --cwd .` -> READY

## 4. Criterio de done

WAVE-7 done cuando:
  - 1791-1795 CLOSED y merged a main
  - `vitest 27/27` + `playwright generative 2/2` PASS
  - `grep -rn Requisitos docs/SRC` 0
  - `curl /health wal_size < 10MB` healthy o degraded (no unhealthy)
  - `gh release view v0.1.1` 3 assets
  - `swal-preflight check` READY

No re-auditar WAVE-5/6 ya verificados. Continuar desde 95c89b96.

## 5. Riesgos

- Jules PR vacío (sin src/): rechazar, pedir fix (guard PR Delivery)
- Conflicto README entre 1795 y main: rebase simple
- WAL fix requiere migración DB: probar con DB vacía y con wal 55MB existente
- Tag push sin 3 builds verdes: no tag hasta WAVE-7 merged

---
*Plan generado 2026-09-02 desde SCAN + WAVE-7 1791-1795 + periferia 4855a0d*
