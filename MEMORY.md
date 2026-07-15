# MEMORY.md — Memoria Persistente del Proyecto Xavier

> Decisiones clave, lecciones aprendidas y notas arquitectónicas.
> Key decisions, lessons learned, and architectural notes.

---

## 🏗️ Decisiones de Arquitectura / Architecture Decisions

### 1. SQLite-Vec como backend vectorial principal
**Fecha:** 2026-03
**Decisión:** Usar SQLite-Vec en lugar de una base de datos vectorial separada (Qdrant, Pinecone).
**Motivo:** Simplicidad operativa — un solo binario, sin servicios externos, backup = copiar un archivo.
**Tradeoff:** Menor rendimiento en búsquedas a muy gran escala (>1M vectores), pero suficiente para ~10K-100K memorias.

### 2. Tokio + Axum para servidor HTTP
**Fecha:** 2026-03
**Decisión:** Axum 0.8 sobre hyper, con Tokio como runtime async.
**Motivo:** Ecosistema Rust moderno, rendimiento, tipo-safe routing.
**Regla crítica:** Nunca llamar `.par_iter()` de Rayon dentro de un worker de Tokio — usar `spawn_blocking`.

### 3. Dual License (AGPL + Commercial)
**Fecha:** 2026-06
**Decisión:** AGPL-3.0 para uso open source, licencia comercial para enterprise.
**Motivo:** Proteger el trabajo manteniendo apertura. Mesh License para features de red y Data Commons.
**Implementado en:** `src/security/license.rs`

### 4. HORMER para navegación jerárquica
**Fecha:** 2026-05
**Decisión:** Sistema de navegación jerárquica con RL policy, Textual Gradient Descent y GRPO.
**Motivo:** Las búsquedas planas no escalan para miles de memorias. La navegación jerárquica permite encontrar información relevante más rápido.
**Estado:** ~90% (Phase 2 en progreso)

### 5. Subagentes con MCP (no REST directo)
**Fecha:** 2026-05
**Decisión:** Exponer memoria vía MCP server (stdio) además de HTTP. Los agentes AGI se conectan por MCP.
**Motivo:** Estandarización — MCP es el protocolo emergente para herramientas de IA. Permite discovery de herramientas.

### 6. Cortex eliminado (simplificación)
**Fecha:** 2026-06
**Decisión:** Remover el plugin de sincronización Cortex completamente. Xavier ahora es el único sistema de memoria.
**Motivo:** Complejidad innecesaria. Un solo sistema de memoria reduce fricción para agentes.

---

## 🧠 Lecciones Aprendidas / Lessons Learned

### 1. Tests primero, siempre
Los tests de integración en `tests/` salvaron el proyecto múltiples veces. Cada refactor grande debe empezar con tests que pasan, luego cambiar código.

### 2. El benchmark Tri-Memory reveló gaps
Comparando Xavier vs Engram vs OpenClaw benchmark (Jun 2026): Xavier gana en búsqueda semántica, Engram en simplicidad MCP, OpenClaw en velocidad. Esto guió features como HORMER y Auto-Improvement.

### 3. No mezclar Tokio y Rayon directamente
La regla de `spawn_blocking` se descubrió por experiencia dolorosa — webhooks colgaban porque BM25 indexing bloqueaba el event loop.

### 4. La documentación se desincroniza rápido
El alignment audit (Jul 2026) encontró ~15% de documentación desactualizada contra el código real. Solución: scripts de verificación automática en CI/CD.

### 5. Telegram bot requiere vault primero
`load_bot_token()` ahora resuelve del vault Clavis primero, después `TELEGRAM_BOT_TOKEN` env var. Mucho más seguro.

---

## 🔮 Planes / Plans

- **Malla P2P real:** Iroh/QUIC transport, NAT traversal, Loro CRDT (Phase 2-4 de Mesh)
- **Auto-Improvement Loop completo:** benchmark → gap analysis → experiment → validate → merge → re-measure
- **Context Regeneration → Perfect Recall:** recall@k → 100% con auto-tuning
- **Governance DAO on-chain:** Solana/Ethereum integration para XIP lifecycle
- **Panel UI estable:** Tauri desktop app para gestión visual de memoria

---

## 📊 Métricas Clave / Key Metrics

| Métrica | Valor | Fuente |
|---------|-------|--------|
| Features totales | 20 | `.gitcore/features.json` |
| Features completadas | 10 | (50%) |
| Features estables | 14 | (70%) |
| Tests que pasan | ~943 | `cargo test` |
| Líneas de código | ~125K | cloc |
| Progreso general | 91% | Reconciliation v0.11.0 |

---

_Última actualización: 2026-07-09_
