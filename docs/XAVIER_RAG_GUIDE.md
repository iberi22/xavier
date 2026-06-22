# Guía Práctica de Xavier RAG para Agentes IA

Esta guía explica cómo conectar cualquier agente de IA (Claude, OpenClaw, DeepSeek, etc.) a Xavier como backend de memoria y RAG en menos de 10 minutos.

## 1. Arranque Rápido

Xavier puede funcionar localmente o en un contenedor. La forma más rápida de empezar es:

### Windows (PowerShell)
```powershell
./start-xavier-rag.ps1
```

### Docker
```bash
docker-compose up -d xavier
```

## 2. Métodos de Integración

### A. Conexión vía MCP (Model Context Protocol)

Este es el estándar recomendado para agentes modernos.

#### Configuración para Claude Desktop / Windsurf / Cursor:
Añade esto a tu configuración de MCP (`mcp_config.json`):

```json
{
  "mcpServers": {
    "xavier": {
      "command": "xavier",
      "args": ["mcp"],
      "env": {
        "XAVIER_TOKEN": "tu-token-aqui"
      }
    }
  }
}
```

### B. Conexión vía HTTP API (Estándar OpenAI)

Si tu agente usa peticiones HTTP, Xavier expone un endpoint compatible con la estructura de memorias estándar.

- **Endpoint de búsqueda**: `POST http://localhost:8006/v1/memories/search`
- **Endpoint de guardado**: `POST http://localhost:8006/v1/memories`

#### Ejemplo en Python:
```python
import requests

XAVIER_URL = "http://localhost:8006/v1/memories/search"
headers = {"X-Xavier-Token": "tu-token-aqui"}

payload = {
    "query": "¿Cuáles son las especificaciones del proyecto?",
    "limit": 5
}

response = requests.post(XAVIER_URL, json=payload, headers=headers)
print(response.json())
```

## 3. Configuración de Embeddings

Xavier soporta tres modos de embeddings:

1. **Local GLLM (Predeterminado)**: Privacidad total, sin coste, ejecutado en tu CPU/GPU.
   - Env: `XAVIER_EMBEDDING_PROVIDER_MODE=local-gllm`
2. **Cloud (OpenRouter/OpenAI)**: Máxima calidad, requiere API Key.
   - Env: `XAVIER_EMBEDDING_PROVIDER_MODE=cloud`
   - Env: `XAVIER_EMBEDDING_URL=https://openrouter.ai/api/v1`
3. **Local Ollama**: Si ya tienes Ollama corriendo.
   - Env: `XAVIER_EMBEDDING_URL=http://localhost:11434/v1/embeddings`

## 4. Verificación de Salud

Para confirmar que Xavier está listo para ser usado por un agente:
```bash
curl http://localhost:8006/v1/health/ready
```

Si el campo `status` es `"ok"`, Xavier está listo para procesar RAG.

## 5. Panel Web (Dashboard)

Visita [http://localhost:8006/panel](http://localhost:8006/panel) para visualizar tus memorias, grafos de conocimiento y el estado del sistema en tiempo real.
