# Transcripción del Premortem: Proyecto Xavier (Cerebro de Memoria Global)
**Fecha:** 2026-07-08

## Contexto Recopilado
- **¿Qué es?** Xavier, el cerebro de memoria global para los agentes SWAL. Un motor de contexto open source en Rust + SQLite-Vec que gestiona persistencia de memoria y mejora continua.
- **¿Para quién es?** Para los agentes de SWAL (codex, opencode, gemini, claude, qwen, Google Jules), BELA (usuario) y el propio Xavier (CEO del sistema).
- **¿Cómo es el éxito?** Un sistema central, ultra rápido y robusto donde todos los agentes sincronizan y recuperan contexto sin bloquear el event loop, asegurando que nada se pierde y todo se reutiliza para evitar trabajo redundante.

---

## 6 Meses Después: El Proyecto ha Fallado

### Razones de Fallo Identificadas (Premortem en bruto)
1. **Bloqueo del Event Loop (Tokio + Rayon):** Computaciones pesadas se ejecutaron en los hilos de trabajo de Tokio sin usar `spawn_blocking`, deteniendo todas las solicitudes HTTP simultáneamente.
2. **Caos de Concurrencia (SQLite-Vec):** Agentes asíncronos y síncronos chocaron escribiendo al mismo tiempo, agotando el pool de conexiones y causando fallos de escritura silenciosos.
3. **Colapso por Ruido de Contexto (Degradación de Búsqueda):** A medida que la base de datos creció, `mem_search` empezó a devolver un volumen inmanejable de decisiones pasadas irrelevantes, rompiendo la confianza de los agentes.
4. **Desincronización Jules vs. CLI Local:** Jules sobreescribió contexto crítico porque los back-fills asíncronos en los PRs no llegaron a tiempo al servidor local.

---

## Análisis Profundo de los Agentes

### Agente 1: Bloqueo del Event Loop
**LA HISTORIA DEL FALLO:** 
En el mes 3, el tamaño del índice BM25 creció. Un agente intentó realizar una indexación masiva mientras que otros 4 agentes (incluyendo a Jules) realizaban llamadas de contexto. Porque un módulo de búsqueda no usaba `tokio::task::spawn_blocking` para la llamada a `par_iter()` de Rayon, los worker threads de Tokio quedaron secuestrados. El servidor de Xavier en el puerto 8006 dejó de responder, perdiendo los Webhooks. Los agentes cayeron en modo offline (degradado) simultáneamente, paralizando todo el sistema SWAL durante días.

**EL SUPUESTO SUBYACENTE:** 
Se asumió que los desarrolladores siempre recordarían y aplicarían la "Regla de Oro" de aislar Rayon de Tokio en cada nuevo endpoint, sin validación automatizada.

**SEÑALES DE ADVERTENCIA TEMPRANAS:**
- Aumento sutil en la latencia de las respuestas del servidor bajo cargas medias.
- Advertencias esporádicas en los logs del servidor sobre tareas de Tokio tardando más de 1 milisegundo.

---

### Agente 2: Caos de Concurrencia (SQLite-Vec)
**LA HISTORIA DEL FALLO:**
Para el mes 4, la cantidad de `create_memory` concurrentes aumentó enormemente a medida que múltiples agentes operaban en paralelo. SQLite-Vec, al manejar concurrencia, comenzó a encontrarse con bloqueos de base de datos (`database is locked`). Dado que el cliente MCP no reintentaba adecuadamente o ignoraba silenciosamente estos errores bajo alta carga, fragmentos cruciales de investigación no se guardaron. Cuando los agentes intentaron recordar decisiones clave la semana siguiente, los datos simplemente no estaban allí, rompiendo por completo la promesa de "memoria persistente".

**EL SUPUESTO SUBYACENTE:**
Se asumió que el pool de conexiones de Rust + SQLite manejaría las colisiones de escritura asíncronas transparentemente bajo alta concurrencia.

**SEÑALES DE ADVERTENCIA TEMPRANAS:**
- Aparición del error `database is locked` en los logs del servidor.
- Agentes reportando que guardaron un fragmento (`save_fragment` exitoso), pero la información no aparecía en búsquedas posteriores.

---

### Agente 3: Colapso por Ruido de Contexto
**LA HISTORIA DEL FALLO:**
A los 6 meses, Xavier tenía miles de nodos de memoria. Cuando un agente consultaba sobre una configuración específica, el límite de `max_chars` se saturaba rápidamente con contexto obsoleto y generalizado de tareas de hace meses, desplazando la decisión arquitectónica reciente y vital. Los agentes comenzaron a tomar decisiones basadas en contexto anticuado, requiriendo intervención humana constante por parte de BELA. El sistema pasó de ser una ventaja a un estorbo que inducía alucinaciones consistentes.

**EL SUPUESTO SUBYACENTE:**
Se asumió que el scoring vectorial (BM25 + embeddings) siempre posicionaría la información reciente y vital por encima de registros voluminosos pero irrelevantes del pasado.

**SEÑALES DE ADVERTENCIA TEMPRANAS:**
- Agentes re-investigando activamente bugs que ya habían sido resueltos y guardados en Xavier la semana anterior.
- Los resúmenes inyectados contenían más de un 50% de información inútil para la tarea actual.

---

### Agente 4: Desincronización Jules vs. CLI Local
**LA HISTORIA DEL FALLO:**
Jules (asíncrono en GitHub) ejecutó una revisión profunda del código y persistió su análisis en la descripción del PR. Sin embargo, el "dispatcher" falló silenciosamente al hacer el back-fill en el servidor local `localhost:8006`. Tres días después, un CLI local síncrono sobrescribió la misma arquitectura sin saber que Jules ya había tomado una decisión estratégica diferente, causando una regresión severa en la rama main.

**EL SUPUESTO SUBYACENTE:**
Se asumió que el puente entre los entornos asíncronos (GitHub PRs) y el cerebro de memoria local era infalible y sin retrasos.

**SEÑALES DE ADVERTENCIA TEMPRANAS:**
- Los PRs fusionados por Jules no reflejaban nuevas memorias creadas en la base de datos de Xavier después de horas de su fusión.

---

## Síntesis: Informe de Premortem

1. **El Fallo Más Probable** — *Bloqueo del Event Loop (Tokio + Rayon).* La tentación de usar iteradores paralelos directamente en endpoints asíncronos de Rust es extremadamente alta, y un solo despiste tumba todo el servidor, bloqueando a todos los agentes de SWAL a la vez.

2. **El Fallo Más Peligroso** — *Colapso por Ruido de Contexto.* Si Xavier deja de proveer información precisa y empuja ruido, los agentes perderán la confianza en el sistema. Una vez que esto pasa, Xavier se convierte en software muerto.

3. **El Supuesto Oculto** — Asumes que la concurrencia de SQLite-Vec es tan robusta como PostgreSQL bajo las ráfagas de escritura masiva que generan agentes LLM autónomos.

4. **El Plan Revisado** 
   - *Para el bloqueo del loop:* Implementar un linter personalizado (o un test de integración agresivo) que detecte llamadas a `.par_iter()` fuera de `spawn_blocking`.
   - *Para el ruido:* Implementar "decaimiento por tiempo" (time decay) en el scoring de búsqueda vectorial, y forzar a los agentes a aplicar filtros estrictos por proyecto/tema en cada búsqueda.

5. **La Lista de Verificación Pre-Lanzamiento**
   - [ ] Ejecutar un script de prueba de estrés que simule 5 agentes asíncronos llamando a `create_memory` 100 veces por segundo para validar los bloqueos de SQLite.
   - [ ] Configurar un sistema de monitoreo simple que registre si alguna tarea del event loop de Tokio tarda más de 5ms.
   - [ ] Validar manualmente que un PR cerrado por Jules inyecta su memoria en `localhost:8006` en menos de 1 minuto.
