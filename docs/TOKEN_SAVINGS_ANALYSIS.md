# Xavier Token Economics — Estudio de Ahorro

## El Problema: Costo de Contexto en Agentes LLM

Cada vez que un agente AI (Claude, ChatGPT, DeepSeek) procesa una consulta,
el **contexto completo de la conversación** suele reenviarse al LLM si el host (por ejemplo, Claude Desktop o VSCode) no gestiona el historial de forma eficiente.

### El Ahorro Honesto de Xavier

Xavier no hace magia de compresión de 99% sobre el aire. El ahorro se consigue mediante **Progressive Disclosure** (Revelación Progresiva) y gestión de capas de memoria.

| Item | Costo SIN Xavier (Full History) | Costo CON Xavier (Optimizado) | Ahorro Real |
|------|-----------------|------------------|--------|
| Último mensaje | 500 tokens | 500 tokens | — |
| Historial (50 turnos) | ~98,000 tokens (reenvío ciego) | ~1,000 tokens (resumen + core slots) | ~99% |
| Archivos del repo | ~5,000 tokens (contexto estático) | ~200 tokens (referencias + snippets) | ~96% |
| **Total por turno** | **~103,500 tokens** | **~1,700 tokens** | **~98.3%** |

**Nota Crítica:** El ahorro del 99% es *teórico* respecto a lo que gastaría un agente sin Xavier si tuviera que leer todo el repo en cada turno. En la práctica, el ahorro depende de la política de reenvío del host. Xavier **garantiza** que el bloque de contexto que él genera es mínimo y suficiente.

## Mecanismo: Progressive Disclosure

Xavier aplica un patrón de "Page-In" similar a la gestión de memoria virtual:

1.  **Search First (Fat Search):** Herramientas como `mem_search` devuelven por defecto solo metadata y snippets (ID, Path, Score). Esto permite al agente ver "qué hay" sin gastar miles de tokens.
2.  **Page-In (Targeted Context):** El agente solo solicita el contenido completo (`memory_context(ids=[...])`) de los documentos que realmente necesita para el paso actual.
3.  **Budget-Aware Selection:** El `Orchestrator` de Xavier selecciona qué mensajes del historial mantener basándose en un budget honesto (Shallow: 50t, Medium: 200t, Deep: 1000t).

## Estimación Honesta de Tokens

Xavier utiliza un estimador conservador para sus reportes de ahorro:
- **Fórmula:** `tokens = ceil(chars / 4)`
- Esto evita el overclaim común de contar espacios o usar métricas optimistas.

## Profundidades de Regeneración

| Nivel | Budget (Tokens) | Contenido | Cuándo usarlo |
|-------|--------|-----------|---------------|
| **Shallow** | ~50 | Metadata básica, última acción, estado de la rama. | Consultas de estado rápido. |
| **Medium** | ~200 | Resumen ejecutivo + 3-5 decisiones clave + archivos críticos. | Debugging estándar. |
| **Deep** | ~1000 | Contexto expandido, grafos de creencia y call paths. | Tareas de arquitectura compleja. |

## Conclusión

Xavier reduce drásticamente el desperdicio de tokens al eliminar la redundancia del historial y los archivos estáticos, sustituyéndolos por una **regeneración dinámica** del contexto necesario para el turno actual.

- **Ahorro típico en debugging:** 90-95%
- **Ahorro en repos grandes:** Hasta 98% mediante Fat Search.
- **Transparencia:** Xavier reporta el uso original vs optimizado en cada restauración.
