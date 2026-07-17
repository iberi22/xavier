# Roadmap: Xavier 100% Local (LLM + Embeddings vía Ollama)

Este documento detalla la visión, el estado actual, las olas de desarrollo y el plan futuro para la iniciativa de ejecución **100% Local** de Xavier (asociada al identificador de característica `feat-local-first`).

---

## 🎯 Visión de la Iniciativa

La iniciativa **Xavier 100% Local** busca habilitar el funcionamiento completo de Xavier de manera local y soberana, garantizando privacidad absoluta, reduciendo costes de nube y operando sin conexión a Internet (offline-first). Esto incluye:
- **Capa de Razonamiento (LLMs locales):** Mediante la integración y orquestación con Ollama y herramientas del sistema.
- **Capa Semántica (Embeddings locales):** Integración nativa a través de modelos locales eficientes (como GLLM o embeddings de Ollama).
- **Capa de Persistencia (Vector DB local):** Almacenamiento y búsqueda híbrida (BM25 + sqlite-vec) completamente locales.
- **Resiliencia & Fallback:** Un sistema robusto de degradación progresiva y elegante, garantizando que el chat responda usando memoria local o fallback a cloud si los proveedores locales fallan.

---

## 🗺️ Tabla de Olas de Progreso

| Ola | Nombre de la Ola | Estado | Descripción |
| :--- | :--- | :--- | :--- |
| **Ola 1** | Estabilización de Capacidad Local | ✅ **DONE** | Compilación limpia del workspace, detección activa de Ollama (`EmbedderConfig::auto()` y sondeos con `is_reachable()`), y endpoints HTTP iniciales. |
| **Ola 2** | Integración & Fallback Elegante | ✅ **DONE** (Cerrado por este issue) | Chat del panel responde usando LLM local con fallback ordenado a Cloud y degradación final a memoria local cuando ningún proveedor responde. |
| **Ola 3** | Observabilidad Avanzada | 📅 **PLANNED** | Métricas de latencia por proveedor, contabilidad de tokens consumidos local vs. cloud, visualización de estado de salud detallado. |
| **Ola 4** | Gestión Dinámica de Modelos (Hot-swap UI) | 📅 **PLANNED** | Interfaz de usuario para descargar y cambiar modelos de Ollama al vuelo desde el panel administrativo de Xavier sin reiniciar el servidor. |

---

## 🔗 Enlaces de Interés

Para configurar, desplegar u optimizar tu instancia local, consulta las siguientes guías detalladas:
- [Guía de Configuración Local (LOCAL_SETUP.md)](LOCAL_SETUP.md) — Instrucciones paso a paso para arrancar Xavier con Ollama y modelos locales.
- [Bridges de LLM Locales (LOCAL_LLM_BRIDGES.md)](LOCAL_LLM_BRIDGES.md) — Alternativas locales como OpenCode CLI, lm-studio u otros proveedores de red.
- [Integración de Embeddings Locales (LOCAL_EMBEDDINGS.md)](LOCAL_EMBEDDINGS.md) — Detalle técnico del motor de embeddings (GLLM vs Ollama) y tests de integración local.

---

## 📦 Detalle de Entregables por Ola

### 🟢 Ola 1: Estabilización de Capacidad Local (PR #540, PR #547, PR #525)
- **Compilación Limpia:** Garantizar que el workspace compile al 100% sin dependencias de red necesarias para compilar.
- **Detección Dinámica:** `EmbedderConfig::auto()` sondea dinámicamente Ollama para determinar capacidades de embedding locales.
- **is_reachable Check:** Métodos rápidos de ping de red para verificar la conectividad con el puerto `11434` de Ollama con un timeout corto de 2 segundos.
- **Endpoints Básicos:** Registro de rutas de salud iniciales.

### 🟡 Ola 2: Integración & Fallback Elegante (Issues 01–13)
- **[issue 01] local como candidato del ProxyUseCase:** Permite que el proxy reconozca el proveedor local como candidato prioritario en la orquestación.
- **[issue 02] fallback chain cableada al ProxyUseCase:** Implementación de la cadena de fallbacks ordenados de Local -> Cloud.
- **[issue 03] degradación a memoria:** Si tanto Local como Cloud fallan, el sistema recurre de manera elegante a responder utilizando la memoria semántica disponible de Xavier.
- **[issue 04] fallback chain al boot:** Configuración y arranque seguro de la cadena de fallbacks al inicializar el servidor.
- **[issue 05] provider local real, no stub:** Implementación del cliente LLM real para conectarse y consumir la API compatible de Ollama.
- **[issue 06] health-check + boot log:** Observabilidad básica del estado del proveedor local y alertas visibles en el log de inicio de Xavier.
- **[issue 07] UI modo:** Selector visual en el panel UI para activar/forzar el modo local-first.
- **[issue 08] memory-fallback UI:** Indicador visual en el chat cuando el sistema entra en modo de degradación elegante a memoria.
- **[issue 09] tests de integración:** Suite de pruebas que verifica de extremo a extremo la cadena de fallbacks y degradaciones.
- **[issue 10] endpoints:** Exposición en la API REST pública de la configuración activa de fallbacks y estados de los proveedores locales.
- **[issue 11] circuit breaker:** Desconexión automática temporal de proveedores lentos o inestables para evitar congelar la interfaz.
- **[issue 12] config local por defecto:** Establecer local-first por defecto en el archivo de configuración inicial de Xavier.
- **[issue 13] docs:** Documentación integral sobre la arquitectura de resiliencia y el uso diario.

### 🔵 Ola 3: Observabilidad Avanzada (Próximamente)
- Métricas de latencia de Ollama en vivo.
- Contador de tokens procesados y ahorro estimado frente a proveedores cloud.
- Alertas proactivas en la UI si Ollama se detiene o el modelo configurado no está descargado.

### 🔵 Ola 4: Gestión Dinámica de Modelos (Próximamente)
- Consola de control para administrar Ollama directamente desde Xavier.
- Capacidad para descargar nuevos modelos de embedding/razonamiento con un solo clic.

---

## 📌 EPIC de Seguimiento en GitHub

Para ver la correlación de issues y el progreso global, el EPIC de referencia sugerido en GitHub es:
> **EPIC: Xavier 100% Local (LLM + embeddings vía Ollama)**

*Nota: Este EPIC agrupa los issues 01 al 13 de la Ola 2 como tareas hijas.*
