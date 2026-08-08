# TASK.md — Sesión activa: preparación de waves W13-W16

> Actualizado: 2026-08-08 · Proyecto: xavier · Rama: main (88adbc01, v0.13.0)

## Estado actual

- v0.13.0 tagueado y pusheado · 36 features (35 stable + 1 planned)
- Suite: 1472 tests passing, 0 failed · verify 36/36 PASS
- Issue Context Packager definido (REQ-028/US-041) pero NO implementado
- edge-hive (#1254) NO compila localmente — pendiente
- Mesh: 0 peers activos — pendiente probar 2 nodos
- Mini-experto: registro + script existen, pipeline Colab SIN probar
- Binario runtime: v0.12.0 (los routers F12 requieren rebuild + restart)

## Tareas en curso

- [x] Cerrar wave F12 (v0.13.0, tag, CHANGELOG, README)
- [x] Definir feat-issue-context-packager (REQ-028/US-041, 36/36 verify)
- [x] Cerrar issues duplicados #1240/#1241/#1242
- [x] Plan de waves W13-W16 en .gitcore/implementation-plan.json
- [x] PLANNING.md con goal verificable para tag v0.15.0
- [ ] Actualizar kanban de Hermes con las tareas de W13-W16
- [ ] Lanzar tests de estabilidad completos (suite + verify)
- [ ] Commit del plan (implementation-plan.json + PLANNING.md + TASK.md)

## Siguiente wave (W13 — a despachar)

ICP-01: parser de issues + mapper CodeGraph
ICP-02: IssueContextPackage + POST /v1/f12/issue-context
ICP-03: benchmark ahorro tokens (≥50%) + docs

## Bloqueos

- Rebuild + restart del binario xavier requiere aprobación del usuario
- edge-hive: shared-protocol build script roto (diagnóstico pendiente)
