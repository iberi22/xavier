# Guía de Usuario Xavier 100% Local (LLM + Embeddings vía Ollama)

Esta guía detalla el proceso completo para configurar y operar **Xavier** de forma totalmente local, privada y soberana, utilizando **Ollama** para la ejecución de Modelos de Lenguaje (LLM) y modelos de incrustaciones de texto (Embeddings).

Con este enfoque, todo el procesamiento se realiza en tu propia infraestructura sin enviar datos a la nube, garantizando máxima privacidad de tu código y datos personales.

---

## 🏗️ 1. Prerrequisitos del Sistema

### Hardware Mínimo y Recomendado
La ejecución local de modelos de IA depende directamente de las capacidades de tu CPU, memoria RAM y tarjeta gráfica (GPU).

| Componente | Requisito Mínimo | Requisito Recomendado | Notas |
| :--- | :--- | :--- | :--- |
| **Procesador (CPU)** | Intel Core i5 / AMD Ryzen 5 (4 núcleos) | Intel Core i7 / AMD Ryzen 7 o Apple Silicon (M1/M2/M3) | CPU con instrucciones AVX2 activas. |
| **Memoria RAM** | 8 GB RAM | 16 GB - 32 GB RAM | Indispensable para cargar los modelos en memoria de forma simultánea. |
| **Tarjeta Gráfica (GPU)** | No requerida (Ejecución en CPU lenta) | NVIDIA RTX con >= 6 GB VRAM o Apple Silicon unificado | Acelera la inferencia exponencialmente. |
| **Almacenamiento** | 10 GB de espacio libre (SSD) | 20 GB de espacio libre (NVMe SSD) | Los modelos locales ocupan entre 2 GB y 8 GB cada uno. |

### Sistemas Operativos Soportados
*   **Linux**: Ubuntu 22.04 LTS o superior, Debian, Arch Linux.
*   **macOS**: macOS 12 Monterey o superior (Optimizado para Apple Silicon M-series).
*   **Windows**: Windows 10/11 de 64 bits con WSL2 (Windows Subsystem for Linux) o ejecutable nativo.

---

## 🦙 2. Instalar y Verificar Ollama

Ollama es un motor de inferencia ligero y fácil de usar diseñado para ejecutar grandes modelos de lenguaje de forma local.

### Instalación Oficial
Sigue las instrucciones según tu sistema operativo:

*   **Linux / macOS**:
    Ejecuta el script de instalación oficial en tu terminal:
    ```bash
    curl -fsSL https://ollama.com/install.sh | sh
    ```
*   **Windows**:
    Descarga el instalador oficial desde [ollama.com/download/windows](https://ollama.com/download/windows) y ejecuta el instalador `.exe`.

### Verificación del Servicio
Asegúrate de que el daemon o servicio de Ollama esté ejecutándose correctamente abriendo una terminal y consultando su estado de red:

```bash
curl http://localhost:11434
```
**Respuesta esperada:**
```text
Ollama is running
```

---

## 📥 3. Descargar Modelos Requeridos

Xavier viene preconfigurado para utilizar dos modelos específicos:
1.  **`qwen3-coder`** (el LLM optimizado para chat y análisis de código).
2.  **`embeddinggemma`** (el modelo de embeddings para búsqueda semántica local).

Descárgalos ejecutando los siguientes comandos en tu terminal:

```bash
ollama pull qwen3-coder
ollama pull embeddinggemma
```

### Tabla de Alternativas para Hardware Limitado
Si tu máquina tiene especificaciones de hardware limitadas (por ejemplo, 8 GB de RAM sin GPU), puedes usar modelos más pequeños compatibles con Xavier:

| Propósito | Modelo por Defecto | Dimensión | Alternativa Ligera | Dimensión Alternativa | Beneficio |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Inferencia / Chat** | `qwen3-coder` | - | `qwen2.5-coder:1.5b` o `llama3.2:1b` | - | Ocupa < 1.5 GB en RAM, inferencia fluida en CPU. |
| **Embeddings** | `embeddinggemma` | 1024 | `nomic-embed-text` o `all-minilm` | 768 / 384 | Menor consumo de memoria y compatibilidad garantizada. |

*Nota: Si utilizas un modelo de embedding alternativo, recuerda que debes reindexar tu base de datos de memoria para adaptar las dimensiones vectoriales ejecutando `xavier reindex`.*

---

## 🚀 4. Instalar y Configurar Xavier

Una vez que Ollama esté listo con sus respectivos modelos, puedes proceder con la instalación de Xavier.

### Instalación de Xavier
*   **A través del instalador interactivo:**
    *   *Windows (PowerShell)*:
        ```powershell
        irm https://raw.githubusercontent.com/iberi22/xavier/main/install.ps1 | iex
        ```
    *   *Linux / macOS*:
        ```bash
        curl -fsSL https://raw.githubusercontent.com/iberi22/xavier/main/install.sh | bash
        ```

*   **Desde el código fuente (Entorno de desarrollo):**
    ```bash
    git clone https://github.com/iberi22/xavier.git
    cd xavier
    cargo build --release
    # El binario compilado estará disponible en target/release/xavier
    ```

### Asistente de Configuración Local (`xavier setup --local`)
Xavier incluye un wizard interactivo para automatizar la configuración del entorno local. Para iniciarlo, ejecuta:

```bash
xavier setup --local
```

Este comando realizará las siguientes tareas de forma automática:
1.  Detectará si Ollama se está ejecutando.
2.  Verificará si tienes descargados los modelos `qwen3-coder` y `embeddinggemma`. Si no los tienes, te ofrecerá descargarlos en ese instante.
3.  Probará la accesibilidad de red de ambos endpoints.
4.  Escribirá la configuración local predeterminada en `config/xavier.config.json` y creará la sección correspondiente en tu archivo `.env`.

#### Configuración generada en el `.env`:
```env
# --- LOCAL-FIRST SETUP ---
XAVIER_MODEL_PROVIDER=local
XAVIER_LOCAL_LLM_URL=http://localhost:11434/v1
XAVIER_LOCAL_LLM_MODEL=qwen3-coder
XAVIER_EMBEDDING_URL=http://localhost:11434/api/embeddings
XAVIER_EMBEDDING_MODEL=embeddinggemma
XAVIER_EMBEDDING_PROVIDER_MODE=local
```

---

## 🏁 5. Arrancar Xavier y Acceder al Panel

### Arrancar el Servidor
Para iniciar el servidor HTTP de Xavier expuesto en el puerto predeterminado 8006, ejecuta el subcomando real:

```bash
xavier http 8006
```

Al arrancar, deberías visualizar un banner informativo confirmando el modo de operación **LOCAL**:
```text
🟢 Xavier iniciado — modo: LOCAL
   LLM:        ollama/qwen3-coder @ http://localhost:11434/v1 [reachable]
   Embeddings: ollama/embeddinggemma @ localhost:11434 [reachable]
   Vector DB:  sqlite_vec (vec-store.sqlite3)
```

### Acceso al Panel de Control
Abre tu navegador de preferencia y accede a la interfaz gráfica:

👉 **[http://localhost:8006/panel](http://localhost:8006/panel)**

### Verificación Visual en la UI
En la barra de estado superior del Panel UI de Xavier, localiza el indicador de estado del proveedor. Deberás ver el badge en verde:

*   **Badge**: `🦙 Local` (o `LOCAL-HEALTHY` indicando que la conexión y tiempos de respuesta de Ollama son correctos).

---

## 🐳 6. Alternativa con Docker (Resiliencia e Infraestructura)

Si prefieres aislar Xavier y sus servicios en contenedores para evitar instalaciones en el sistema anfitrión, puedes utilizar la configuración de Docker.

1.  Asegúrate de tener Docker y Docker Compose instalados en tu máquina.
2.  Levanta la pila de servicios local utilizando el archivo compose del repositorio:
    ```bash
    docker compose up -d
    ```
3.  Este contenedor montará por defecto la base de datos de vectores en un volumen persistente (`xavier_data`) y configurará la comunicación con tu instancia de Ollama local a través del host de red de Docker (`host.docker.internal`).

Para más detalles técnicos sobre despliegues automatizados, consulta [docs/DOCKER_DEPLOY.md](DOCKER_DEPLOY.md) y [docs/DEPLOYMENT.md](DEPLOYMENT.md).

---

## 📊 Diagrama del Flujo de Inferencia y Fallbacks

El siguiente diagrama detalla cómo viaja una consulta del usuario desde la interfaz gráfica, atravesando el sistema de proxies securizados, intentando la ejecución en Ollama local y degradándose con resiliencia si es necesario:

```mermaid
graph LR
  A[panel UI] --> B[/panel/api/chat]
  B --> C[ProxyUseCase]
  C --> D{Ollama local}
  D -- ok --> E[Respuesta LLM]
  D -- falla --> F[Cloud fallback]
  F -- falla --> G[Memory fallback]
```

*   **Cloud Fallback:** Si tienes configurado un proveedor en la nube (como OpenAI/Anthropic) en tu archivo de configuración o cadena de fallbacks, Xavier intentará desviar la consulta allí si Ollama falla.
*   **Memory Fallback:** Si ningún proveedor de lenguaje responde, el sistema recurre al modo offline de degradación semántica profunda, formulando una respuesta basada exclusivamente en la búsqueda de documentos e hilos archivados en la base de datos vectorial local (`sqlite-vec`).

---

## 🔄 7. Gestión de Modelos Ollama (Hot-Swap)

Xavier introduce endpoints de plano de control para gestionar dinámicamente los modelos de Ollama desde el panel de administración o mediante API en caliente (hot-swap), permitiendo descargar y activar modelos sin tener que reiniciar el servidor.

1. **Listar modelos disponibles en la instancia local (`GET /v1/ollama/models`)**:
   Devuelve los modelos que están actualmente descargados en tu instancia de Ollama.
   *   **Ejemplo de Petición**:
       ```bash
       curl -X GET http://localhost:8006/v1/ollama/models \
         -H "X-Xavier-Token: TU_XAVIER_TOKEN"
       ```
   *   **Ejemplo de Respuesta**:
       ```json
       {
         "status": "ok",
         "models": [
           {
             "name": "qwen3-coder:latest",
             "size": 4721453210,
             "digest": "sha256:d8a23...",
             "details": {
               "format": "gguf",
               "family": "qwen2",
               "parameter_size": "7B",
               "quantization_level": "Q4_K_M"
             }
           },
           {
             "name": "embeddinggemma:latest",
             "size": 1712415120,
             "digest": "sha256:f123c...",
             "details": {
               "format": "gguf",
               "family": "gemma",
               "parameter_size": "2B"
             }
           }
         ]
       }
       ```

2. **Descargar un nuevo modelo (`POST /v1/ollama/pull`)**:
   Permite solicitar a Ollama la descarga/pull de un nuevo modelo en segundo plano.
   *   **Ejemplo de Petición**:
       ```bash
       curl -X POST http://localhost:8006/v1/ollama/pull \
         -H "Content-Type: application/json" \
         -H "X-Xavier-Token: TU_XAVIER_TOKEN" \
         -d '{"name": "llama3.2:3b"}'
       ```
   *   **Ejemplo de Respuesta**:
       ```json
       {
         "status": "downloading",
         "message": "Iniciada la descarga del modelo llama3.2:3b en segundo plano"
       }
       ```

3. **Cambiar el modelo local activo en caliente (`POST /v1/ollama/active`)**:
   Establece el modelo LLM activo para las siguientes conversaciones en Xavier.
   ⚠️ **Nota de rendimiento**: Este comando actualiza de forma dinámica las variables de entorno de configuración interna del proceso (`process env`). Por lo tanto, **no se requiere un reinicio completo del servidor**; la próxima petición de chat que envíes utilizará el nuevo modelo seleccionado de inmediato de forma transparente.
   *   **Ejemplo de Petición**:
       ```bash
       curl -X POST http://localhost:8006/v1/ollama/active \
         -H "Content-Type: application/json" \
         -H "X-Xavier-Token: TU_XAVIER_TOKEN" \
         -d '{"model": "qwen3-coder"}'
       ```
   *   **Ejemplo de Respuesta**:
       ```json
       {
         "status": "ok",
         "active_model": "qwen3-coder",
         "message": "Modelo local actualizado dinámicamente para las siguientes peticiones de chat"
       }
       ```

---

## 📈 8. Métricas de Uso

Xavier cuenta con un sistema de observabilidad de alta precisión que registra en tiempo real el consumo de recursos tanto para proveedores en la nube como para inferencia local y caídas a memoria de recuperación (fallbacks).

Puedes consultar de forma unificada estas métricas y cuotas consumidas utilizando el endpoint `/v1/account/usage`.

*   **Ejemplo de Petición**:
    ```bash
    curl -X GET http://localhost:8006/v1/account/usage \
      -H "X-Xavier-Token: TU_XAVIER_TOKEN"
    ```

*   **Estructura de la Respuesta**:
    El endpoint devuelve un desglose completo de peticiones, tokens procesados, costes acumulados y métricas de resiliencia del sistema:
    ```json
    {
      "status": "ok",
      "requests_used": 142,
      "total_tokens": 87450,
      "total_errors": 4,
      "total_cost_usd": 0.0423,
      "memory_fallback_hits": 12,
      "fallback_chain_hops": 3,
      "by_provider": {
        "local": {
          "requests": 130,
          "tokens_in": 45000,
          "tokens_out": 38000,
          "errors": 1,
          "cost_usd": 0.0
        },
        "openai": {
          "requests": 12,
          "tokens_in": 2450,
          "tokens_out": 2000,
          "errors": 3,
          "cost_usd": 0.0423
        }
      },
      "provider_quotas": {
        "local": {
          "provider": "local",
          "limit_tokens": null,
          "used_tokens": 83000,
          "is_blocked": false,
          "cooldown_until": null
        },
        "openai": {
          "provider": "openai",
          "limit_tokens": 100000,
          "used_tokens": 4450,
          "is_blocked": false,
          "cooldown_until": null
        }
      },
      "optimization": {
        "router_direct_count": 0,
        "semantic_cache_hits": 15,
        "semantic_cache_misses": 127
      }
    }
    ```

*   **Campos Clave a Monitorear**:
    *   `requests_used` / `total_tokens`: Volumen total procesado por el orquestador.
    *   `total_cost_usd`: Coste monetario de las llamadas a nube (es `0.0` para las llamadas al proveedor `local`/Ollama, reflejando el ahorro real).
    *   `memory_fallback_hits`: Número de veces que el sistema tuvo que responder usando la base de datos de memoria vectorial local al no haber ningún LLM disponible.
    *   `fallback_chain_hops`: Saltos automáticos realizados en la cadena de fallbacks (ej. reintento redirigido a local tras fallo de cloud).
    *   `by_provider`: Desglose detallado de peticiones, tokens de entrada/salida, errores y costes por cada proveedor individual.

---

## 🛠️ 9. Troubleshooting (Solución de Problemas)

A continuación se listan los 5 casos de error más comunes al utilizar Xavier en modo 100% local y cómo solucionarlos:

### Caso 1: Ollama no arranca o puerto 11434 ocupado
*   **Síntoma:** Error de conexión rechazada al iniciar Ollama o al intentar hacer `ollama pull`.
*   **Causa:** Otra instancia de Ollama, un servidor web o un contenedor de Docker está escuchando en el puerto predeterminado `11434`.
*   **Solución:**
    *   *Linux/macOS*: Detéctalo corriendo `lsof -i :11434` y finaliza el proceso colgado con `kill <PID>`. Alternativamente, puedes forzar una IP o puerto alternativo mediante la variable de entorno `OLLAMA_HOST=127.0.0.1:11435` antes de lanzar el servicio.
    *   *Windows*: Cierra la aplicación de bandeja del sistema de Ollama (System Tray icon) y vuelve a lanzarla.

### Caso 2: El Chat siempre cae a "memory-fallback" o no responde
*   **Síntoma:** El chat del panel responde instantáneamente con un prefijo `[Modo memoria — LLM no disponible]` y un badge de disquete `💾` en el panel de mensajes.
*   **Causa:** Xavier no puede comunicarse con el LLM configurado. Posiblemente el modelo `qwen3-coder` no está instalado en Ollama, la URL del host local es incorrecta, o el puerto está cerrado.
*   **Solución:**
    1.  Ejecuta `xavier doctor` (o `xavier verify health`) para diagnosticar el estado del servidor.
    2.  Verifica los modelos disponibles de Ollama en tu sistema local corriendo `curl http://localhost:11434/api/tags` o `ollama list`.
    3.  Asegúrate de que `XAVIER_LOCAL_LLM_MODEL` en tu archivo `.env` coincide exactamente con el nombre impreso en la lista de Ollama.

### Caso 3: Búsqueda extraña o nula tras cambiar el modelo de Embeddings
*   **Síntoma:** Realizas búsquedas en el panel o el sistema de consulta local no devuelve coincidencias relevantes después de cambiar de `embeddinggemma` a `nomic-embed-text` (o viceversa).
*   **Causa:** Los vectores almacenados en `vec-store.sqlite3` fueron generados con las dimensiones o pesos del modelo anterior, provocando incompatibilidad dimensional o semántica (mismatch).
*   **Solución:**
    *   Debes forzar una reindexación total de tu base de datos de memoria para generar nuevos vectores basados en el modelo actual. Para ello, realiza una petición POST al endpoint de reindexado:
        ```bash
        curl -X POST http://localhost:8006/memory/reindex \
          -H "X-Xavier-Token: TU_XAVIER_TOKEN"
        ```
    *   O bien desde la terminal con el comando CLI nativo de Xavier:
        ```bash
        xavier reindex
        ```

### Caso 4: Volver a modo Cloud (Nube)
*   **Síntoma:** Deseas suspender la ejecución local y volver a utilizar proveedores de nube comerciales de alta disponibilidad (como GPT-4o de OpenAI o Claude 3.5 Sonnet de Anthropic).
*   **Solución:**
    1.  Puedes cambiar el proveedor activo en caliente ejecutando el CLI:
        ```bash
        xavier provider set openai
        ```
    2.  O bien edita tu archivo `.env` y ajusta las siguientes variables:
        ```env
        XAVIER_MODEL_PROVIDER=openai
        OPENAI_API_KEY=tu_clave_de_api_aquí
        ```
    3.  Reinicia el servidor de Xavier (`xavier http 8006`).

### Caso 5: El Panel UI muestra error 401 (Token incorrecto o no autorizado)
*   **Síntoma:** Al abrir `http://localhost:8006/panel`, la pantalla queda en blanco, muestra un error de autenticación "401 Unauthorized" o falla al realizar llamadas API.
*   **Causa:** El token utilizado por el panel web o las llamadas CLI no coincide con el definido en el backend de Xavier.
*   **Solución:**
    1.  Genera un nuevo token seguro de forma local ejecutando:
        ```bash
        xavier token new
        ```
    2.  Exporta el token en tu terminal antes de lanzar el servidor:
        ```bash
        export XAVIER_TOKEN="el_token_generado"
        ```
    3.  En Windows PowerShell, hazlo de la siguiente manera:
        ```powershell
        $env:XAVIER_TOKEN="el_token_generado"
        ```
    4.  Asegúrate de que este mismo token esté guardado en la variable `XAVIER_TOKEN` dentro del archivo `.env`.

---

## ❓ 10. FAQ (Preguntas Frecuentes)

### Q1: ¿Necesito obligatoriamente una GPU dedicada para ejecutar Xavier local?
**R:** No. Ollama y Xavier pueden ejecutarse al 100% en CPU (utilizando la memoria RAM del sistema). No obstante, los tiempos de generación de texto (tokens por segundo) serán significativamente inferiores comparados con un sistema acelerado por hardware GPU.

### Q2: ¿Funciona Xavier de forma completamente offline (sin internet)?
**R:** Sí. Una vez que has descargado Ollama, Xavier y los modelos requeridos (`qwen3-coder` y `embeddinggemma`), puedes desconectar tu equipo de internet por completo. Xavier operará de forma autónoma almacenando las memorias en su base de datos local SQLite y realizando inferencia offline.

### Q3: ¿Cuánto espacio en disco ocupará esta instalación en total?
**R:** La instalación básica de Xavier ocupa menos de 50 MB. Sin embargo, los modelos descargados vía Ollama requieren almacenamiento considerable:
*   `qwen3-coder` (tamaño aproximado de 4.5 GB)
*   `embeddinggemma` (tamaño aproximado de 1.7 GB)
Se recomienda disponer de un mínimo de 10 GB a 15 GB de almacenamiento libre en un disco de estado sólido (SSD) para un rendimiento óptimo.

### Q4: ¿Xavier recopila telemetría o envía información a algún servidor externo?
**R:** Por defecto, **no**. Xavier es un software local-first enfocado en la soberanía de los datos. Existe un módulo opcional denominado "Data Commons" que permite el envío de datos consentidos y cifrados para fines de entrenamiento, pero está completamente desactivado por defecto y requiere una activación expresa por parte del usuario.

### Q5: ¿Cómo puedo actualizar los modelos locales a sus versiones más recientes?
**R:** Puedes indicarle a Ollama que descargue la versión más reciente de cualquier modelo local volviendo a ejecutar el comando de descarga:
```bash
ollama pull qwen3-coder
ollama pull embeddinggemma
```

### Q6: ¿Por qué Xavier utiliza dos modelos en lugar de uno solo para el chat y la búsqueda?
**R:** La arquitectura RAG (Generación Aumentada por Recuperación) de Xavier separa las responsabilidades para optimizar el rendimiento. Un modelo de embeddings dedicado (como `embeddinggemma`) se especializa en convertir textos en vectores numéricos de forma ultra-rápida y precisa para indexar tu base de conocimiento. Por su parte, un modelo generativo de lenguaje (como `qwen3-coder`) se encarga de leer el contexto recuperado y redactar respuestas coherentes y estructuradas al usuario.
