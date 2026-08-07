# FEEDBACK WAVE XAVIER — 2026-08-06 (integración con subagente OpenCode qwen3.8-max)

## PRs integrados (3/3 MERGED en main, 16:10Z)
| PR | Título | Issue | Estado | Commit merge |
|----|--------|-------|--------|--------------|
| #1213 | feat(panel-ui): refactor MalocaView to use swal-maloca-panel custom element | #1212 (U.10) | MERGED | aef6b491 |
| #1214 | Palette: Add accessible form labels to Cloud Relay Config | — | MERGED | b44fd6b9 |
| #1215 | Bolt: Memoize InputArea callbacks to prevent unnecessary re-renders | — | MERGED | 732c4c93 |

## CI local (resultados reales)
- pnpm typecheck panel-ui: 0 errores
- pnpm build panel-ui: EXIT 0 (build → dist OK)
- vitest panel-ui: suite presente (tests existentes: inputArea.a11y, graphAdapters, auth, roadmapGraph, operationModeBadge)
- cargo check -p xavier: sin errores tras el fix de build
- Fix de build commiteado: 0492c469 (wire cross-repo deps @swal/maloca-embed + maloca-wasm)
- Push a origin/main: OK, local == origin (0492c469)

## PROBLEMAS ENCONTRADOS (hallazgos para próximas waves)
1. **PR #1213 rompía el build** por deps cross-repo no resueltas:
   - `@swal/maloca-embed` (file:../../maloca/packages/swal-maloca-embed) depende de `@swal/ui@workspace:*` que NO estaba en el workspace de xavier → pnpm error WORKSPACE_PKG_NOT_FOUND.
   - `@swal/maloca-wasm` importa `./pkg/maloca_wasm.js` pero `pkg/` está GITIGNORED en maloca → pnpm file: no lo copia → UNRESOLVED_IMPORT.
   - maloca-wasm/main.js importa default export de wasm-bindgen async, pero el pkg compilado es sync (solo named exports) → MISSING_EXPORT en build.
   - Fix aplicado (3 capas): añadir paquetes maloca al pnpm-workspace.yaml de xavier + alias de vite @swal/maloca-wasm→path real + assetsInclude wasm + external en rollupOptions para que el fallback TS runtime del import dinámico funcione.
2. **Falta el dist de maloca-embed en CI**: el paquete maloca-embed depende de que maloca tenga su build hecho (dist/). Orden de build cross-repo necesario.
3. **Test de MalocaView no entregado**: el PR #1213 borró panel-ui/tests/malocaVote.test.ts sin reemplazo — el wrapper del custom element NO tiene tests nuevos.
4. **features.json de xavier** (.gitcore/features.json) sin reconciliar tras U.10 — pendiente para wave de cierre.

## Pendientes para nuevas waves
- [ ] Tests del wrapper MalocaView (custom element swal-maloca-panel) — reemplazar malocaVote.test.ts borrado
- [ ] Reconciliación .gitcore/features.json xavier (U.10 MalocaView + endpoints /maloca/* ya en main)
- [ ] Restart manual xavier.service con binario nuevo (EPIC #1198 recall σ>0) — one-liner entregado al usuario
- [ ] Build wasm de maloca-wasm (pkg/ gitignored) — decidir si commitea artifact o se construye en CI cross-repo
