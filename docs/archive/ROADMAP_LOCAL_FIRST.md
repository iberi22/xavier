# Roadmap: Xavier 100% Local (LLM + Embeddings vía Ollama)

Este documento detalla la visión, el estado actual, las olas de desarrollo y el plan futuro para la iniciativa de ejecución **100% Local** de Xavier (`feat-local-first`, EPIC GitHub **#522**).

**Última reconciliación:** 2026-07-18 · `features.json` overall **~95%** · `feat-local-first` **100%** stable

---

## Visión de la Iniciativa

La iniciativa **Xavier 100% Local** busca habilitar el funcionamiento completo de Xavier de manera local y soberana, garantizando privacidad absoluta, reduciendo costes de nube y operando sin conexión a Internet (offline-first). Esto incluye:

- **Capa de Razonamiento (LLMs locales):** Ollama y bridges compatibles OpenAI.
- **Capa Semántica (Embeddings locales):** GLLM / embeddings Ollama.
- **Capa de Persistencia (Vector DB local):** BM25 + sqlite-vec.
- **Resiliencia & Fallback:** degradación Local → Cloud → Memoria.
- **Observabilidad local-first:** doctor, health, status MCP/Telegram, métricas (en curso).

---

## Tabla de Olas de Progreso

| Ola | Nombre | Estado | Descripción |
| :--- | :--- | :--- | :--- |
| **Ola 1** | Estabilización de Capacidad Local | ✅ **DONE** | Compilación limpia, `EmbedderConfig::auto()`, `is_reachable()`, endpoints base. |
| **Ola 2** | Integración & Fallback Elegante | ✅ **DONE** | Proxy local, fallback chain, memory degradation, UI modo, tests, config default. |
| **Ola 3** | Observabilidad & Hardening | ✅ **DONE** | Circuit breaker, doctor, health, reindex, panel LLM, MCP/Telegram status, Docker/docs/smoke, **UsageCounters (#578)**. EPIC formal #589 en cierre. |
| **Ola 4** | Gestión Dinámica + paridad | ✅ **DONE** | #619 Ollama API · #615 metrics UI · #622 headless fallback + Ollama UI + e2e + docs. `feat-local-first` **100%**. |

---

## Enlaces de Interés

- [USER_GUIDE_LOCAL.md](USER_GUIDE_LOCAL.md) — Guía usuario final 100% local (Ola 3).
- [LOCAL_SETUP.md](LOCAL_SETUP.md) — Setup Ollama + Xavier.
- [LOCAL_LLM_BRIDGES.md](LOCAL_LLM_BRIDGES.md) — Bridges alternativos.
- [LOCAL_EMBEDDINGS.md](LOCAL_EMBEDDINGS.md) — Embeddings locales.
- [OLA4_ANALYSIS.md](OLA4_ANALYSIS.md) — Análisis formal de la siguiente ola.
- `.gitcore/features.json` · `.gitcore/features-detailed.json`

---

## Detalle por Ola

### Ola 1 — Estabilización (DONE)
- Compilación limpia del workspace.
- Detección dinámica Ollama (`EmbedderConfig::auto()`).
- `is_reachable` con timeout corto.
- Endpoints de salud base.

### Ola 2 — Integración & Fallback (DONE)
Issues 01–13 (local provider, fallback chain, memory degradation, boot, UI, tests, endpoints, circuit breaker legacy, config default, docs).

### Ola 3 — Observabilidad & Hardening (13/14 DONE)

| Issue | Entregable | PR | Estado |
| ---: | :--- | ---: | :--- |
| #576 | Circuit breaker por provider | #592 | ✅ |
| #577 | Reindex al cambiar embedding model | #597 | ✅ |
| **#578** | **Métricas de uso reales** | orchestrator | ✅ UsageCounters |
| #579 | `xavier doctor` | #600 | ✅ |
| #580 | docker-compose.local | #593 | ✅ |
| #581 | USER_GUIDE_LOCAL | #591 | ✅ |
| #582 | `/health` enriquecido | #599 | ✅ |
| #583 | MCP `xavier_local_status` | #604 | ✅ |
| #584 | Telegram `/localstatus` | #604 | ✅ |
| #585 | E2E chat fallback | #596 | ✅ |
| #586 | Log retention + boot event | #595 | ✅ |
| #587 | Config fail-fast | #594 | ✅ |
| #588 | Smoke scripts | #598 | ✅ |
| #590 | Panel STUB → LLM real | #605 | ✅ |
| #589 | EPIC cierre features/ROADMAP/devlog | — | 🔒 blocked by #578 |

**Notas de integración Ola 3:**
- PRs Jules #601/#602 se cerraron por bloat (56 archivos); reemplazo quirúrgico #604.
- Panel chat ya no es stub: `execute_secured` + memory fallback.
- `cargo check --workspace` y `cargo check --features telegram` en verde post-integración.

### Ola 4 — Plan (ver [OLA4_ANALYSIS.md](OLA4_ANALYSIS.md))

**Fase 0 — Cerrar Ola 3 (bloqueante)**
1. Implementar #578 `UsageCounters` reales en ProxyUseCase + `/v1/usage`.
2. Cerrar #589: features 100% local-first band, ROADMAP, devlog.

**Fase 1 — Producto local-first**
3. Hot-swap de modelos Ollama desde panel (pull/list/select sin reinicio).
4. Paridad headless: memory-fallback en `headless_chat` (hoy panel sí, headless no).
5. Superficie de métricas en panel UI (tokens local vs cloud).

**Fase 2 — Backlog transversal (priorizable)**
6. #497 Progressive disclosure MCP (P0 cost).
7. #478 Dependabot triage security.
8. #445 code-graph FTS5.

---

## EPIC de Seguimiento en GitHub

| Issue | Rol |
| ---: | :--- |
| **#522** | EPIC iniciativa 100% Local |
| **#578** | Única deuda funcional Ola 3 |
| **#589** | EPIC cierre formal Ola 3 |
| **#590** | Panel STUB (cerrado vía #605) |

---

## Criterio de “100%” para `feat-local-first`

| Criterio | Estado |
| :--- | :--- |
| LLM local usable vía ProxyUseCase | ✅ |
| Fallback cloud + memoria | ✅ panel y headless con memory-fallback explícito |
| Circuit breaker | ✅ |
| Doctor + health + local_status | ✅ |
| Docker + docs + smoke | ✅ |
| Métricas tokens/providers/fallback | ✅ #578 (UsageCounters) |
| Hot-swap modelos UI | ✅ (Ola 4 hot-swap completo) |
| features.json + ROADMAP + devlog al día | ✅ completo |

Todo completado con éxito: **feat-local-first → 100% DONE** estable.
