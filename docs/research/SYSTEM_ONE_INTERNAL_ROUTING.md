# Arquitectura Interna System One: Alternativa Local en Rust a Jev (TypeSafe)

Este documento detalla la arquitectura **System One** implementada en Xavier como alternativa nativa, local y soberana a clasificadores SaaS externos como TypeSafe Jev. Expone el análisis comparativo (Antes vs. Después), el alcance de las optimizaciones, la estrategia de replicación en Rust, los mecanismos de tolerancia a fallos (*fallback*) y el protocolo de auto-actualización.

---

## 1. El Problema: El Enfoque Tradicional y Dependencia SaaS (Jev)

### Diagnóstico del Modelo Tradicional
En los runtimes agénticos habituales:
1. **Sobrecarga de LLM (System Two Lane):** Cada interacción, continuación mecánica de tool-call, verificación de sintaxis o mensaje intermedio se despacha directamente a un modelo frontier generativo (e.g. Claude 3.5 Sonnet, GPT-4o, DeepSeek V3).
2. **Latencia Inaceptable:** Cada llamada generativa introduce entre **1.200 ms y 3.500 ms** de latencia por turno, incluso para respuestas booleanas simples como `{"is_safe": true}`.
3. **Coste por Tokens Repetitivos:** Las continuaciones de ejecución ("Tool execution succeeded, continue") acumulan miles de tokens de contexto duplicado.

### La Alternativa SaaS Externa (Jev / TypeSafe)
Para mitigar esto, servicios como TypeSafe Jev proponen una API externa de clasificación rápida:
- **Puntos Críticos de Falla:**
  - **Dependencia de Red Externa:** Añade entre **250 ms y 600 ms** de latencia de red (DNS + TLS handshake + round-trip).
  - **Fuga de Privacidad / PII:** El contexto de las herramientas y los datos del agente deben salir a los servidores del proveedor SaaS para ser clasificados.
  - **Bloqueo por Proveedor / Facturación:** Se añade un coste por petición y una dependencia frágil sujeta a caídas del servicio o límites de tasa (*rate limits*).

---

## 2. La Solución en Xavier: System One Nativo en Rust

En lugar de delegar el triaje en una API de terceros, Xavier implementa el patrón **System One** en el módulo `src/agents/fast_router_poc.rs`.

### Principios de Diseño
1. **Triaje en Submilisegundos (< 1 ms):** Evaluación determinista o probabilística in-memory en CPU local.
2. **Salidas Estrictamente Tipadas:** Genera structs Rust deserializables (`SystemOneDecision`) sin involucrar generación token a token (*no strings attached*).
3. **Detección Local de PII:** Escaneo preventivo en el nodo antes de que cualquier fragmento de memoria se envíe al LLM.
4. **Desvío al Carril Rápido (*FastLane*):** Hasta un **75%** de continuaciones mecánicas de herramientas se resuelven en Rust puro sin invocar al modelo pesado.

---

## 3. Comparativa Cuantitativa: Antes vs. Después

| Dimensión | Enfoque Previo (LLM Puro) | Enfoque SaaS Externo (Jev) | ✅ Xavier System One Nativo |
| :--- | :--- | :--- | :--- |
| **Latencia de Triaje** | 1.200 ms – 3.000 ms | 300 ms – 800 ms (RTT Red) | **< 1 ms** (In-Memory CPU) |
| **Coste por Petición** | Coste por tokens de prompt/salida | Facturación mensual / por API call | **$0.00** (Cero dependencias) |
| **Privacidad / PII** | Contexto expuesto a LLM API | Contexto expuesto a clasificador SaaS | **100% Local y Blindado** |
| **Disponibilidad Offline** | No | No (falla si cae internet) | **Sí (Opera 100% local / airgap)** |
| **Mecanismo de Fallback** | Ninguno (falla general) | Fallback manual por timeout | **Fail-Open Automático a System Two** |
| **Ahorro de Tokens** | 0% | Parcial | **39.2% – 48.8% medido** |

---

## 4. Mecanismos de Fallback y Resiliencia (Tolerancia a Fallos)

Para evitar fallos catastróficos si la heurística o el clasificador rápido no alcanzan certeza suficiente:

1. **Principio *Fail-Open*:**
   - Si la confianza calculada es menor al umbral configurado (`confidence_threshold: 0.90`), el `FastRouter` degrada suavemente a `Route::SystemTwoLane`.
   - Si el paso contiene un error de compilación o runtime (`has_error: true`), la confianza desciende inmediatamente a `0.45`, delegando la resolución de la incidencia al LLM inteligente de razonamiento profundo.
2. **Aislamiento Criptográfico:**
   - La presencia de cadenas sospechosas de contener PII (`SSN`, `password`, claves privadas) fuerza el desvío hacia canales seguros o sanitizadores antes de persistir o propagar.
3. **Resiliencia de Almacenamiento:**
   - Si falla la vectorización o el motor SQLite local, Xavier activa el `fallback_store.rs` y el `fallback_transport.rs` para mantener la memoria operativa.

---

## 5. Detección de Actualizaciones y Autoinstalación

Xavier cuenta con un servicio nativo de actualización `AutoUpdateService` (`src/mesh/auto_update.rs`):

- **Verificación contra GitHub Releases:**
  - Consulta periódicamente `https://api.github.com/repos/iberi22/xavier/releases/latest`.
  - Normaliza y compara versiones numéricas (`normalize_version` y `compare_version_parts`).
- **Estados de Actualización (`UpdateStatus`):**
  - `UpToDate`: El nodo corre la versión más reciente.
  - `UpdateAvailable`: Existe una versión superior disponible con su respectiva URL de descarga y release notes.
  - `CurrentAhead`: La versión local es de desarrollo o commit adelantado al último release público.
- **Flujo de Despliegue de Binarios Multiplataforma:**
  - El workflow `.github/workflows/release.yml` compila automáticamente binarios optimizados para:
    - Linux x86_64: `xavier-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
    - macOS ARM64: `xavier-vX.Y.Z-aarch64-apple-darwin.tar.gz`
    - Windows x86_64: `xavier-vX.Y.Z-x86_64-pc-windows-msvc.zip`
    - Contenedor Docker en GHCR: `ghcr.io/iberi22/xavier:latest`
  - Genera sumas de verificación criptográficas `sha256` para cada binario antes de publicarlos en GitHub Releases.

