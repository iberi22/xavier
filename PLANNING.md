# Xavier — Implementation Planning

> Protocolo GitCore · Actualizado 2026-08-08 · Versión actual: v0.13.0
> Fuente de verdad: `.gitcore/implementation-plan.json`

## 🎯 Goal de la serie (verificable — habilita tag v0.15.0)

**v0.15.0 — Xavier operativo de punta a punta:**

1. `POST /v1/f12/issue-context` devuelve `IssueContextPackage` con `PreciseChange`
   de un issue real de GitHub (benchmark ≥50% ahorro de tokens)
2. edge-hive compila y se registra en `/v1/f12/directory`
3. Un mini-experto GGUF responde vía Ollama (entrenado con datos reales de
   Xavier vía Colab CLI)
4. 2 nodos xavier conectados (`active_peers ≥ 1`)
5. verify-pipeline 38/38 PASS + suite completa 0 failed

**El tag v0.15.0 se aplica SOLO cuando se cumplan los 5 criterios.**

## Serie de waves

| Wave | Nombre | Issues | Meta |
|------|--------|--------|------|
| **W13** | Issue Context Packager + cierre deuda | ICP-01..03 | El análisis del issue queda LISTO (línea exacta) antes de delegar |
| **W14** | Nodo SWAL real + mesh 2 nodos | NODO-01..02, MESH-01 | edge-hive compila y se conecta; 2 xaviers conversan |
| **W15** | Mini-experto real (Colab) | ME-01..02 | GGUF entrenado con datos propios responde localmente |
| **W16** | Estabilización final + release | REL-01..02 | Runtime real + tag v0.15.0 |
| **W17** | Compilación/checks delegados a Jules (GPU local en uso) | JULES-01..03 | Todo cargo build/check/test corre en sandbox Jules — cero ejecución local |
| **W18** | Puesta a punto funcional — LLM, ICP, mesh, mini-experts, release | XAV-01..07 | Xavier de local-degraded → healthy + v0.15.0: OpenRouter activo, ICP 100%, mesh-service-network, edge-hive, mini-experto GGUF, snapshot manager, release |

## Estado por feature

| Feature | Estado | Wave |
|---------|--------|------|
| feat-issue-context-packager | 🔎 planned 0% | W13 |
| feat-mini-experts | ⏳ 100% (código) — pipeline Colab sin probar | W15 |
| feat-mesh-network (edge-hive) | ⏳ #1254 abierto | W14 |
| feat-mesh-network (2 nodos) | ⏳ 0 peers activos | W14 |
| feat-context-regeneration | ✅ 100% — recall@k benchmark | — |
| 33 features restantes | ✅ stable | — |

## Decisiones de versión

- **v0.13.0** (2026-08-08): +8 features F12, snapshot manager, routers HTTP
- **NO v1.0.0**: prematuras las condiciones (ICP sin existir, mesh sin peers,
  edge-hive roto, Colab sin probar). Los tags v1.0.0/v1.0.0-rc.1 fueron eliminados.
- **v0.15.0**: la meta de esta serie (goal_verifiable arriba)

## Verificación de estabilidad

Cada wave se cierra con:
- `cargo test -p xavier --lib` → 0 failed
- `bash .gitcore/scripts/verify-pipeline.sh` → N/N PASS
- Smoke test de endpoints nuevos en runtime (tras rebuild aprobado)
