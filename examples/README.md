# Xavier Agent Integration Examples

Este directorio contiene ejemplos de cómo integrar Xavier con diferentes agentes de IA.

## MCP (Model Context Protocol)

### Claude Desktop
Copia el contenido de `claude_desktop_config.json` en tu archivo de configuración de Claude Desktop (usualmente en `%APPDATA%\Claude\claude_desktop_config.json` en Windows).

### OpenClaw
OpenClaw soporta Xavier nativamente. Asegúrate de configurar la variable de entorno `XAVIER_TOKEN`.

## REST API

### `python_rag_client.py`
Ejemplo simple de cómo realizar búsquedas semánticas y guardar memorias usando Python.

### `nodejs_rag_client.js`
Ejemplo de integración con Node.js usando `fetch`.
