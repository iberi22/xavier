# Transcripción del Premortem: Proyecto Xavier

## Contexto Recopilado
* **¿Qué es?**: Xavier es un framework avanzado para la gestión, orquestación y sincronización de memoria de agentes de IA (como OpenClaw), que incluye un sistema de sincronización híbrido (local/nube con Supabase/Postgres), un ecosistema de plugins (vía cargo/MCP), observabilidad vía Telegram, y un frontend de control (panel-ui).
* **¿Para quién es?**: Equipos y desarrolladores que operan múltiples agentes de IA autónomos, que requieren persistencia de memoria distribuida, integraciones seguras y observabilidad en tiempo real.
* **¿Cómo es el éxito?**: (Proyección a Diciembre / 1 Año) Adopción masiva del ecosistema de plugins, sincronización perfecta y sin pérdidas de memoria entre nodos, superación de todos los benchmarks (tri-memory LoCoMo), y estabilidad del sistema bajo alta concurrencia operativa.

---

## Razones de Fallo Generadas (Premortem en bruto)
1. **Pérdida de Datos por Concurrencia (LWW):** La estrategia "Last Writer Wins" (LWW) en `CloudMemorySync` provocó la sobrescritura y destrucción de memorias críticas cuando múltiples agentes OpenClaw intentaron actualizar el estado simultáneamente.
2. **Rechazo del Ecosistema de Plugins (Barrera de Rust):** El requisito de usar `cargo` para el ciclo de vida de los plugins externos limitó drásticamente la adopción. La comunidad de IA (mayoritariamente en Python/Node) se negó a lidiar con toolchains de Rust, matando el ecosistema de extensiones.
3. **Bloqueos Irreversibles por Seguridad Inflexible:** El sistema estricto de recuperación local (semilla BIP39 en español y 10 códigos de un solo uso) resultó ser demasiado propenso a errores humanos para entornos empresariales; administradores clave perdieron acceso, dejando instancias críticas de Xavier inaccesibles.
4. **Sobreoptimización de Benchmarks vs. Realidad:** La recuperación de memoria (RRF/BM25) fue altamente optimizada para pasar las 35 consultas estructuradas de `tri_memory_queries.json`, pero en producción, con el lenguaje natural caótico de los usuarios, la recuperación semántica falló estrepitosamente, volviendo a los agentes amnésicos en contextos del mundo real.

---

## Análisis Profundos de los Agentes

### Agente 1: Pérdida de Datos por Concurrencia
**Historia del Fallo:**
Para diciembre, Xavier fue desplegado en una empresa operando 20 agentes OpenClaw colaborativos. Durante un evento de alta carga, varios agentes actualizaron sus `belief_states` e indexaron nuevos chunks en `MEMORY.md` simultáneamente. Debido a la arquitectura de `CloudMemorySync`, que utiliza una estrategia pasiva de *Last Writer Wins* impulsada por el `node_id`, las operaciones de escritura concurrentes sobrescribieron los progresos de las demás. Semanas de memoria contextual fueron aniquiladas silenciosamente sin emitir alertas, causando que los agentes repitieran tareas y perdieran coherencia, lo que llevó al cliente a abandonar la plataforma.

**Supuesto Subyacente:**
El equipo asumió que los conflictos de escritura en memorias de IA serían raros y que un simple "último en escribir gana" sería suficiente, ignorando la naturaleza hiper-concurrente de los sistemas multi-agente.

**Señales de Advertencia Tempranas:**
1. Los logs diarios de los agentes (`.openclaw/`) muestran desajustes frecuentes frente al estado persistido en Supabase/Postgres.
2. Picos de eventos `update` en el `SessionEvent` sin el correspondiente contexto ampliado (los registros simplemente se reemplazan, no se fusionan).

### Agente 2: Rechazo del Ecosistema de Plugins
**Historia del Fallo:**
Un año después del lanzamiento, la pestaña de "Plugins" en el `panel-ui` seguía vacía a excepción de `codegraph`. Cuando se promocionó el sistema MCP, los desarrolladores descubrieron que el ciclo de vida de instalación en `src/plugin_manager.rs` requería invocar `cargo install`. Los ingenieros de ML, acostumbrados a `pip` y `npm`, se negaron a instalar la cadena de herramientas de Rust, lidiar con errores de compilación o resolver dependencias en entornos de contenedor. Sin plugins de terceros, Xavier quedó como una herramienta aislada, perdiendo contra frameworks más fáciles de extender.

**Supuesto Subyacente:**
Asumieron que la base de usuarios de orquestación de IA tendría la misma afinidad (o tolerancia) por Rust que los creadores de Xavier.

**Señales de Advertencia Tempranas:**
1. Usuarios abriendo issues de GitHub pidiendo soporte directo para Docker/Python MCP servers en lugar de integraciones vía `cargo`.
2. Tiempo de despliegue prolongado y fallos continuos durante el proceso guiado de `xavier setup` al fallar dependencias del sistema operativo.

### Agente 3: Bloqueos Irreversibles por Seguridad Inflexible
**Historia del Fallo:**
Para el mes 6, varios de los clientes iniciales más grandes perdieron acceso total a su base de datos local y panel de control. El sistema utiliza una semilla de 12 palabras (BIP39 en español) y códigos alfanuméricos en SQLite. Durante una rotación de administradores, los códigos se perdieron, y las frases semilla no fueron respaldadas correctamente. Como el sistema es "local-only" para la recuperación, el equipo de Xavier no pudo ayudarles a recuperar el acceso. La frustración escaló en foros públicos, marcando a Xavier como "inseguro para la continuidad del negocio".

**Supuesto Subyacente:**
Se dio por hecho que los administradores de sistemas corporativos tratarían las credenciales de Xavier con el mismo rigor que las billeteras de criptomonedas (hardware cold storage).

**Señales de Advertencia Tempranas:**
1. Aumento drástico de llamadas de soporte de usuarios preguntando por una opción de "resetear contraseña" basada en email.
2. Uso anormal del endpoint `/auth/recovery/backup-codes` debido al pánico y extravío constante de la semilla de 12 palabras.

### Agente 4: Sobreoptimización de Benchmarks vs. Realidad
**Historia del Fallo:**
El equipo celebró en el mes 3 cuando `scripts/benchmark_tri_memory.py` reportó resultados muy superiores a Engram. Sin embargo, en diciembre, los usuarios reportaron que los agentes "alucinaban" porque el `QmdMemory` fallaba en recuperar contexto crítico. El sistema RRF (Reciprocal Rank Fusion) con el fallback estricto de BM25 había sido afinado paramétricamente (TTL, normalización de OpenAI) solo para los 7 escenarios y 35 consultas del LoCoMo del repositorio. Ante el lenguaje ambiguo y contextual del chat humano-agente, el buscador no cruzaba los umbrales de relevancia y devolvía arreglos vacíos o zonas (`zone`) incorrectas.

**Supuesto Subyacente:**
Creer que superar un dataset sintético estructurado y cerrado (benchmark_tri_memory) equivale a resolver el problema dinámico del "recall" de memoria en la práctica.

**Señales de Advertencia Tempranas:**
1. Gran divergencia entre las métricas verdes de las simulaciones en `--mock` / `--live` y el aumento de tickets de usuarios sobre la "amnesia" del agente.
2. Alta tasa de caída al "BM25 fallback" en logs de producción debido a puntajes semánticos muy bajos en búsquedas complejas.

---

## Síntesis (Informe)

1. **El Fallo Más Probable:**
   El rechazo del ecosistema de plugins debido al requisito de `cargo`. El mercado actual de agentes IA está dominado por Node y Python; forzar una cadena de compilación de Rust como método oficial de gestión de plugins matará rápidamente los incentivos de la comunidad para integrar MCPs con Xavier.

2. **El Fallo Más Peligroso:**
   Pérdida silenciosa de datos debido a la sincronización en nube "Last Writer Wins" (LWW). Si la memoria es el "alma" (`SOUL.md`, `MEMORY.md`) del agente, destruirla aleatoriamente por condiciones de carrera arruina inmediatamente el valor fundamental (trust) del sistema entero.

3. **El Supuesto Oculto:**
   Creer que el entorno de desarrollo y operación del creador (entusiastas de Rust, obsesionados con criptografía BIP39 y benchmarks rigurosos) es exactamente el mismo que el entorno de producción ruidoso y descuidado del usuario final empresarial.

4. **El Plan Revisado:**
   - *Refactorizar CloudMemorySync:* Cambiar LWW por un modelo de sincronización basado en CRDTs, o al menos un versionado explícito con resolución manual de conflictos para evitar sobrescrituras de `memory_records`.
   - *Desacoplar Plugins:* Permitir que el `PluginManager` configure servidores MCP externos simplemente apuntando a binarios precompilados, scripts de Python/Node o contenedores Docker en el `XavierSettings`, eliminando la dependencia dura de `cargo install`.
   - *Red de Seguridad de Auth:* Implementar una función opcional de recuperación "Admin Override" basada en tokens de infraestructura (ej. un secreto atado al despliegue de base de datos) para casos en los que se pierde la frase semilla.
   - *Testing Real de Memoria:* Integrar datos de sesiones orgánicas y ciegas en el flujo de CI, no solo las 35 consultas rígidas de `tri_memory_queries.json`.

5. **La Lista de Verificación Pre-Lanzamiento:**
   - [ ] Hacer un test de estrés de 100 escrituras por segundo desde 3 instancias (nodos) simuladas al backend de Supabase para validar si LWW pierde transacciones, documentando la tasa de fallo.
   - [ ] Probar instalar un plugin en un sistema operativo completamente limpio (una VM de Ubuntu/Windows sin `rustup`) para verificar qué sucede.
   - [ ] Hacer un simulacro en el que el desarrollador principal pierde sus códigos y su semilla; intentar recuperar la cuenta utilizando herramientas de bajo nivel de PostgreSQL/Supabase.
   - [ ] Inyectar 500 consultas extraídas de chats humanos reales a través de `POST /v1/memory/search` para analizar la degradación del BM25 fallback frente al benchmark.