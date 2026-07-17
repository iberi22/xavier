# Guía Xavier Local-First (Ollama)

Esta guía explica cómo configurar Xavier para funcionar de manera 100% local utilizando [Ollama](https://ollama.com/), tras las mejoras implementadas en la Ola 2 del proyecto.

Xavier ahora implementa una arquitectura local-first nativa donde el chat del panel utiliza automáticamente el LLM y los embeddings locales, gestionando de forma fluida la redundancia y el modo de operación degradado.

---

## 🚀 Post-Ola 2: Operación 100% Local (Quickstart)

El flujo de inicio rápido para tener a Xavier operando de manera offline y local se resume en instalar Ollama, descargar los modelos optimizados y arrancar el servicio.

### Prerrequisitos

*   **Ollama**: Descarga e instala Ollama desde [ollama.com](https://ollama.com/).
*   **Servicio Activo**: Asegúrate de que el daemon de Ollama esté corriendo (`ollama serve`). Puedes verificarlo abriendo `http://localhost:11434` en tu navegador o mediante curl.

### Setup de 3 Pasos

Ejecuta los siguientes comandos en tu terminal para preparar los modelos y arrancar Xavier:

1.  **Descargar el modelo LLM local para chat y razonamiento:**
    ```bash
    ollama pull qwen3-coder
    ```

2.  **Descargar el modelo local para embeddings semánticos:**
    ```bash
    ollama pull embeddinggemma
    ```

3.  **Iniciar el servidor de Xavier:**
    ```bash
    xavier serve
    ```
    *(Nota: Si estás en un entorno de desarrollo, puedes usar `cargo run -- serve`)*

---

## 🔍 Verificación del Sistema

### 1. Boot Log (Consola)
Al arrancar el servidor, Xavier realiza un escaneo de capacidades del sistema (incluyendo la detección de Ollama y sus modelos). El log de inicialización en la consola debe confirmar el correcto funcionamiento mostrando un banner similar al siguiente:

```text
🟢 Xavier iniciado — modo: LOCAL
   LLM:        ollama/qwen3-coder @ http://localhost:11434/v1 [reachable]
   Embeddings: ollama/embeddinggemma @ localhost:11434 [reachable]
   Vector DB:  sqlite_vec (vec-store.sqlite3)
```

Este log indica que ambos servicios locales están en estado `[reachable]` y listos para procesar inferencias.

### 2. Panel UI Badge
Una vez iniciado el servidor, accede a la interfaz gráfica. En el selector de proveedores o estado del sistema, deberías observar el siguiente indicador visual:

*   **Badge**: `🦙 Local` (en verde, indicando que se está utilizando la inferencia local saludable).

---

## 🔄 Resiliencia y Fallback Automático

Xavier ha sido diseñado para no interrumpir el flujo de trabajo del usuario ante caídas de proveedores.

1.  **Cadena de Fallbacks con Cloud (Prioridad Mixta):**
    Si decides configurar tanto proveedores cloud (como OpenAI o Anthropic) como locales, la cadena de fallbacks interna los organiza de manera que los servicios cloud se evalúen primero y el local sirva como respaldo último (o viceversa si forces modo local estricto).
2.  **Transición Transparente:**
    Si el proveedor principal configurado (ej. OpenAI) experimenta una interrupción o agota su cuota de peticiones, Xavier redirige la petición de chat de forma completamente automática y transparente al backend local de Ollama (`qwen3-coder`).

---

## 💾 Degradación Gradual (Modo Degradado)

¿Qué pasa si incluso Ollama o el hardware local fallan? Xavier implementa una degradación elegante y robusta para evitar respuestas en blanco o bloqueos:

1.  **Transición a Local Degradado:**
    Si los endpoints de Ollama dejan de responder tras varios intentos, el monitor de salud (`HealthMonitor`) cambia el estado operacional de la aplicación a `local-degraded`.
2.  **Badge Visual en la UI:**
    El indicador del panel se actualiza para mostrar el estado:
    *   **Badge**: `⚠️ Degradado` (en amarillo, alertando de la indisponibilidad del motor de inferencia local).
3.  **Respuestas desde Memoria:**
    En este estado, el chat no fallará con errores de conexión. En su lugar, el orquestador activará el fallback de memoria profunda. Generará una respuesta contextualizada directamente desde los documentos calientes, resúmenes episódicos y engramas de la base de datos de vectores local (`sqlite-vec`), acompañando la respuesta con un distintivo visual claro:
    *   **Nota en UI**: `💾` (indicador de que la respuesta ha sido recuperada de la base de datos de memoria persistente offline).

---

## ⚙️ Referencia de Configuración (Variables de Entorno)

La configuración de Xavier se gestiona a través de variables de entorno (definidas en tu archivo `.env`). Asegúrate de que coincidan exactamente con la especificación de configuración local:

```env
# Proveedor principal de inferencia (valores: local, cloud, opencode, etc.)
XAVIER_MODEL_PROVIDER=local

# URL del endpoint de LLM local (Ollama expone un API compatible con OpenAI en /v1)
XAVIER_LOCAL_LLM_URL=http://localhost:11434/v1

# Nombre exacto del modelo de lenguaje descargado en Ollama
XAVIER_LOCAL_LLM_MODEL=qwen3-coder

# Modo de proveedor de embeddings (valores: local para Ollama, local-gllm para nativo Candle, cloud)
XAVIER_EMBEDDING_PROVIDER_MODE=local

# URL del endpoint de embeddings de Ollama
XAVIER_EMBEDDING_URL=http://localhost:11434/api/embeddings

# Nombre exacto del modelo de embeddings descargado en Ollama
XAVIER_EMBEDDING_MODEL=embeddinggemma
```

---

## 🛠️ Solución de Problemas (Troubleshooting)

### Validar Estado de Ollama mediante API
Si sospechas que Ollama no responde, ejecuta el siguiente comando en tu terminal para listar los modelos que tiene cargados la instancia local:
```bash
curl http://localhost:11434/api/tags
```
Deberías recibir una respuesta en formato JSON que contenga los modelos `qwen3-coder` y `embeddinggemma`.

### El Badge de la UI indica `⚠️ Degradado`
1.  Verifica que el servicio de Ollama esté ejecutándose en segundo plano (`lsof -i :11434` o `Get-Process ollama` en Windows).
2.  Asegúrate de que no haya un problema de puerto ocupado por otra instancia o base de datos.
3.  Confirma que has descargado exactamente los nombres de los modelos correspondientes en las variables de entorno de tu archivo `.env`.

### Cambiar de Proveedor en Caliente (Hot-Swapping)
Puedes forzar el cambio de proveedor de inferencia de Xavier en cualquier momento de dos formas:

1.  **Vía CLI de Xavier:**
    ```bash
    xavier provider set local
    ```
2.  **Vía HTTP API Endpoint:**
    Realiza una petición POST al endpoint del servidor Xavier para cambiar el proveedor activo:
    ```bash
    curl -X POST http://localhost:8006/v1/provider/set \
      -H "Content-Type: application/json" \
      -H "X-Xavier-Token: TU_TOKEN_DE_AUTORIZACION" \
      -d '{"provider": "local"}'
    ```

---

## 🔗 Enlaces Relacionados

*   **Embeddings Locales:** Para más detalles sobre el funcionamiento de los embeddings locales, la comparativa entre Ollama y el modo nativo GLLM (Candle), consulta la [Guía de Embeddings Locales](LOCAL_EMBEDDINGS.md).
*   **Puentes de LLM Locales:** Si deseas usar alternativas como LM Studio o el bridge opencode CLI, consulta la [Guía de Puentes LLM](LOCAL_LLM_BRIDGES.md).
*   **Roadmap de Desarrollo:** Explora la visión a largo plazo para una infraestructura offline en el [Roadmap Local-First](ROADMAP_LOCAL_FIRST.md).
