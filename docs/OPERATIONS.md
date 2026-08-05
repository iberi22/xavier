# Xavier System Operations Runbook (Guía de Operaciones)

Este documento detalla las tareas de administración, despliegue, configuración y resolución de problemas para mantener la salud operativa del ecosistema **Xavier**. Cubre el ciclo de vida de los servicios de Large Language Models (LLM) locales (Ollama), la transición automática y manual al sistema de fallback (OpenRouter/OpenAI), la optimización del consumo de tokens y el runbook de recuperación frente a caídas.

---

## 📋 Tabla de Contenidos
1. [Levantar y Verificar Ollama (Modelos Locales)](#1-levantar-y-verificar-ollama-modelos-locales)
2. [Modelo Local vs. Fallback en la Nube (OpenRouter)](#2-modelo-local-vs-fallback-en-la-nube-openrouter)
3. [Optimización de Heartbeats y Consumo de Tokens](#3-optimización-de-heartbeats-y-consumo-de-tokens)
4. [Runbook de Recuperación de Servicios (systemd)](#4-runbook-de-recuperación-de-servicios-systemd)
5. [Checklist de Salud Diaria](#5-checklist-de-salud-diaria)

---

## 1. Levantar y Verificar Ollama (Modelos Locales)

El motor local por defecto de Xavier depende de **Ollama** ejecutándose en segundo plano. Cuando Ollama está caído, el endpoint `/health` de Xavier reportará el LLM como `unhealthy`.

### 1.1 Iniciar y Verificar el Proceso de Ollama

#### En Linux (systemd)
```bash
# Comprobar el estado del servicio
systemctl status ollama

# Iniciar el servicio si está detenido
sudo systemctl start ollama

# Detener el servicio
sudo systemctl stop ollama

# Reiniciar el servicio
sudo systemctl restart ollama
```

#### En Windows o Mac (Proceso de Usuario)
Si se ejecuta como aplicación de escritorio, verifica el proceso utilizando la CLI o el Administrador de Tareas:
```bash
# Buscar procesos activos de Ollama
pgrep -af ollama
```
Si no está ejecutándose, inícialo desde el terminal o la UI:
```bash
ollama serve > ollama_serve.log 2>&1 &
```

### 1.2 Probar Endpoints de Ollama de Forma Directa
Comprueba si Ollama responde correctamente en su puerto estándar (`11434`):

```bash
# 1. Comprobar conectividad general y versión
curl http://localhost:11434/api/version

# 2. Listar modelos locales descargados
curl http://localhost:11434/api/tags
```

### 1.3 Gestionar los Modelos Requeridos
Xavier requiere un modelo de lenguaje (LLM) y un modelo de embeddings locales en su configuración base:
- **LLM por defecto:** `qwen3-coder` (u otro modelo local compatible como `llama3`)
- **Embedder recomendado:** `embeddinggemma`, `nomic-embed-text` o `mxbai-embed-large-v1`

#### Descargar modelos manualmente:
```bash
# Descargar el modelo de generación de código/texto
ollama pull qwen3-coder

# Descargar el modelo de embeddings local
ollama pull embeddinggemma
```

#### Probar la API de Embeddings de Ollama:
Para comprobar que el modelo de embeddings local funciona correctamente antes de levantar Xavier, ejecuta:
```bash
curl -X POST http://localhost:11434/api/embeddings \
  -H "Content-Type: application/json" \
  -d '{"model": "embeddinggemma", "prompt": "Xavier Local Embedder Verification Test"}'
```
*Respuesta esperada:* Un JSON con la clave `"embedding"` que contiene un vector de punto flotante de longitud 768.

---

## 2. Modelo Local vs. Fallback en la Nube (OpenRouter)

Xavier cuenta con un sistema de fallback nativo que conmuta de forma transparente entre la ejecución local (Ollama) y el procesamiento en la nube (OpenRouter/OpenAI) si el servicio local no responde o le faltan modelos requeridos.

### 2.1 Variables de Entorno de Configuración

| Variable de Entorno | Tipo / Valor | Descripción |
|---------------------|--------------|-------------|
| `XAVIER_EMBEDDING_PROVIDER_MODE` | `local` \| `local-gllm` \| `cloud` \| `auto` \| `disabled` | Modo del proveedor de embeddings. |
| `XAVIER_EMBEDDING_LOCAL_URL` | `http://localhost:11434/v1/embeddings` | URL del endpoint de embeddings local (Ollama). |
| `XAVIER_EMBEDDING_URL` | `https://openrouter.ai/api/v1/embeddings` | URL del proveedor de embeddings en la nube (fallback). |
| `XAVIER_EMBEDDING_MODEL` | `embeddinggemma` \| `text-embedding-3-small` | Modelo de embeddings a utilizar por el proveedor. |
| `XAVIER_OPENROUTER_API_KEY` | `sk-or-...` | Token de acceso para la API de OpenRouter. |
| `OPENAI_API_KEY` | `sk-proj-...` | Token de acceso para OpenAI (toma precedencia sobre OpenRouter). |
| `XAVIER_MODEL_PROVIDER` | `local` \| `cloud` | Indica el proveedor primario para el LLM. |
| `XAVIER_LOCAL_LLM_URL` | `http://localhost:11434/v1` | URL base de la API local de LLM. |
| `XAVIER_LOCAL_LLM_MODEL` | `qwen3-coder` | Modelo local configurado para la inferencia de lenguaje. |

### 2.2 Flujo del Fallback de Embeddings (Auto-Fallback)

Cuando `XAVIER_EMBEDDING_PROVIDER_MODE` se define como `auto` o se deja sin definir, Xavier realiza la siguiente lógica secuencial:

1. **Sondeo de Ollama (Reachable Probe):**
   Xavier envía una petición HTTP corta a `http://localhost:11434/v1/models` para verificar la conectividad de Ollama.
2. **Chequeo de Modelos:**
   - Si Ollama responde y tiene `embeddinggemma` instalado, se activa el modo **Local**.
   - Si Ollama responde pero **no** tiene `embeddinggemma` instalado, emitirá una alerta/advertencia (visible vía `xavier doctor` y alertas de sistema) pero mantendrá la intención de ejecución local.
3. **Activación de Fallback Cloud:**
   - Si Ollama **no** es alcanzable y existen credenciales en el entorno (`XAVIER_OPENROUTER_API_KEY` o `OPENAI_API_KEY`), Xavier conmuta automáticamente al proveedor en la nube.
   - Utilizará el modelo **`text-embedding-3-small`** (con dimensión aplanada a 1536) en el endpoint de OpenRouter (`https://openrouter.ai/api/v1/embeddings`) o OpenAI de forma transparente.
4. **Modo No-Op:**
   - Si no hay servicios locales disponibles y no se configuran claves en el entorno, el sistema iniciará en modo degradado con el codificador `NoopEmbedder` (dimensión `0`).

### 2.3 Forzar Modos Específicos de Operación

#### Configuración Exclusiva Local (Sin Fallback Externo):
```bash
export XAVIER_EMBEDDING_PROVIDER_MODE=local
export XAVIER_EMBEDDING_MODEL=embeddinggemma
export XAVIER_EMBEDDING_LOCAL_URL=http://localhost:11434/v1/embeddings
```

#### Configuración Exclusiva Cloud (OpenRouter / OpenAI):
```bash
export XAVIER_EMBEDDING_PROVIDER_MODE=cloud
export XAVIER_EMBEDDING_MODEL=text-embedding-3-small
export XAVIER_EMBEDDING_URL=https://openrouter.ai/api/v1/embeddings
export XAVIER_OPENROUTER_API_KEY=sk-or-tu-api-key-aqui
```

---

## 3. Optimización de Heartbeats y Consumo de Tokens

### 3.1 El Problema del Token Burn en QwenCloud / OpenClaw
Cuando Xavier está integrado con agentes coordinados mediante gateways externos (como QwenCloud u OpenClaw), los agentes envían peticiones de "heartbeat" periódicas para mantener la sesión y el estado de liviandad activos.

Por defecto, el heartbeat de un gateway en segundo plano puede ejecutarse cada **30 minutos**, consumiendo llamadas a la API de inferencia (tokens) de manera continua aunque la plataforma esté inactiva. Esto provoca un desgaste acelerado de los planes de suscripción y saldo (Token Plan).

### 3.2 Recomendaciones de Ajuste (Ops Runbook)
Para atenuar esta fuga de tokens en entornos de producción, se recomienda configurar el intervalo de latido del agente a un espectro más amplio o desactivar el gateway durante períodos de inactividad programada.

#### Ajustar la Configuración de Latidos:
Busca la propiedad `heartbeat` o `agents.defaults.heartbeat.every` en el archivo de configuración global (`config/xavier.config.json` o `.openclaw` de tus sub-agentes) y modifícala:

```json
{
  "agents": {
    "defaults": {
      "heartbeat": {
        "every": "2h"
      }
    }
  }
}
```
*Al subir el valor de `30m` a `1h` o `2h`, se reduce el consumo de tokens pasivos entre un 50% y un 75%.*

#### Detener el Gateway cuando no esté en Uso:
Si estás fuera de horario operativo, detén el puente del agente para evitar el drenaje del plan de tokens:
```bash
# Apagar gateway de OpenClaw/QwenCloud
# (Dependiendo de tu despliegue de agentes)
pkill -f openclaw-gateway
```

---

## 4. Runbook de Recuperación de Servicios (systemd)

Si Xavier deja de responder en el puerto HTTP de la API central (puerto por defecto `3000` o `XAVIER_MCP_PORT`), sigue este runbook estructurado para devolver el sistema a un estado saludable.

### Paso 1: Analizar el Estado del Servicio en systemd
```bash
# Obtener estado detallado del servicio xavier
systemctl status xavier

# Consultar los últimos logs generados por el servicio
journalctl -u xavier -n 100 --no-pager
```

### Paso 2: El Servicio Está "Activo" pero no Responde (Socket Bloqueado)
Si `systemctl` reporta el servicio como activo pero no hay respuesta en la API, es probable que el puerto esté bloqueado por un proceso huérfano o un hilo colgado.

```bash
# 1. Comprobar qué proceso está escuchando en el puerto 3000
lsof -i :3000

# 2. Encontrar el Identificador del Proceso (PID) de Xavier
pgrep -af xavier
```

### Paso 3: Procedimiento de Parada Forzada y Limpieza
Si el servicio no responde a `systemctl stop xavier`, realiza una finalización forzosa del subproceso:

```bash
# Detener el servicio a nivel de sistema
sudo systemctl stop xavier 2>/dev/null || true

# Matar cualquier proceso residual del binario xavier
sudo kill -9 $(pgrep -f xavier) 2>/dev/null || true

# Liberar el socket TCP del puerto si sigue en estado TIME_WAIT o ocupado
sudo kill -9 $(lsof -t -i :3000) 2>/dev/null || true
```

### Paso 4: Levantar y Verificar
```bash
# Iniciar nuevamente el servicio
sudo systemctl start xavier

# Monitorizar el proceso de inicio en tiempo real
journalctl -u xavier -f
```

### Paso 5: Fallback de Arranque Manual (Modo Standalone)
Si el entorno systemd está corrupto o experimenta fallos de permisos, puedes levantar Xavier en modo manual aislándolo en segundo plano:

```bash
# Asegurar variables de entorno requeridas
export XAVIER_DATA_DIR="/opt/xavier/data"
export XAVIER_EMBEDDING_PROVIDER_MODE="auto"

# Ejecutar el binario de forma independiente guardando logs
nohup ./xavier --mcp-port 0 > /var/log/xavier_standalone.log 2>&1 &

# Comprobar que está en ejecución
ps aux | grep xavier
```

---

## 5. Checklist de Salud Diaria

El administrador del sistema debe ejecutar este checklist de salud al inicio de cada jornada para asegurar la resiliencia y el rendimiento óptimo del sistema.

### ▢ 1. Ejecutar el Comando de Diagnóstico Local (`xavier doctor`)
La utilidad integrada realiza auditorías automáticas de la base de datos, accesibilidad al LLM y consistencia de embeddings.
```bash
xavier doctor --verbose
```
*Validación:* Todas las filas de salida cruciales deben marcar **`[✓] OK`**. Si se detecta un mismatch en los embeddings (`Embedding Model Consistency` en `WARN`), planifica una re-indexación de memorias o ajusta `XAVIER_EMBEDDING_MODEL`.

### ▢ 2. Verificar el Estado del Índice de Código (CodeGraph)
```bash
xavier code status
```
*Validación:* Confirma que el número de ficheros indexados coincide con los de la rama de trabajo principal y que el archivo de base de datos de CodeGraph (`data/code_graph.db`) no esté corrupto ni vacío (`total_symbols > 0`).

### ▢ 3. Monitorizar el Uso de Disco de las Bases de Datos SQLite
Las bases de datos SQLite en producción acumulan registros y fragmentación.
```bash
# Comprobar el tamaño de la base de datos de vectores de memoria
ls -lh /opt/xavier/data/xavier_memory_vec.db

# Comprobar la base de datos de auditorías de seguridad
ls -lh /opt/xavier/data/security.db
```
*Mantenimiento preventivo:* Si el tamaño es excesivo, ejecuta una compactación manual o el proceso de consolidación nocturna:
```bash
# Realizar compactación y purga de expiraciones nocturnas de Xavier
xavier memory consolidate
```

### ▢ 4. Verificar Disponibilidad de Puertos y Endpoints HTTP
```bash
# Validar endpoint de salud general
curl -s http://localhost:3000/health | jq .

# Validar endpoint de readiness
curl -s http://localhost:3000/readiness | jq .

# Validar el dashboard del Mesh (comunicación peer-to-peer y latencias)
curl -s -H "Authorization: Bearer <TU_WORKSPACE_TOKEN>" http://localhost:3000/v1/mesh/health | jq .
```

### ▢ 5. Inspeccionar Consumo de Memoria RAM de Ollama y Xavier
```bash
free -m
ps -eo pid,ppid,cmd,%mem,%cpu --sort=-%mem | head -n 10
```
*Validación:* Asegúrate de que las capas de aceleración por GPU no están sobrecargando la memoria del host provocando que el kernel invoque el Out-Of-Memory Killer (`OOM-killer`).
