# Xavier RAG Practical Guide for AI Agents

This guide explains how to connect any AI agent (Claude, OpenClaw, DeepSeek, etc.) to Xavier as a memory and RAG backend in under 10 minutes.

## 1. Quick Start

Xavier can run locally or in a container. The fastest way to start is:

### Windows (PowerShell)
```powershell
./start-xavier-rag.ps1
```

### Docker
```bash
docker-compose up -d xavier
```

## 2. Integration Methods

### A. Connection via MCP (Model Context Protocol)

This is the recommended standard for modern agents.

#### Configuration for Claude Desktop / Windsurf / Cursor:
Add this to your MCP config (`mcp_config.json`):
```json
{
  "mcpServers": {
    "xavier": {
      "command": "xavier",
      "args": ["mcp"],
      "env": {
        "XAVIER_TOKEN": "your-token-here"
      }
    }
  }
}
```

### B. Connection via HTTP API (OpenAI Standard)

If your agent uses HTTP requests, Xavier exposes an endpoint compatible with standard memory structures.

- **Search endpoint**: `POST http://localhost:8006/v1/memories/search`
- **Save endpoint**: `POST http://localhost:8006/v1/memories`

#### Python Example:
```python
import requests

XAVIER_URL = "http://localhost:8006/v1/memories/search"
headers = {"X-Xavier-Token": "your-token-here"}

payload = {
    "query": "What are the project specifications?",
    "limit": 5
}

response = requests.post(XAVIER_URL, json=payload, headers=headers)
print(response.json())
```

## 3. Embedding Configuration

Xavier supports three embedding modes:

1. **Local GLLM (Default)**: Full privacy, no cost, runs on your CPU/GPU.
   - Env: `XAVIER_EMBEDDING_PROVIDER_MODE=local-gllm`
2. **Cloud (OpenRouter/OpenAI)**: Highest quality, requires API Key.
   - Env: `XAVIER_EMBEDDING_PROVIDER_MODE=cloud`
   - Env: `XAVIER_EMBEDDING_URL=https://openrouter.ai/api/v1`
3. **Local Ollama**: If you already have Ollama running.
   - Env: `XAVIER_EMBEDDING_URL=http://localhost:11434/v1/embeddings`

## 4. Health Check

To confirm Xavier is ready for agent use:
```bash
curl http://localhost:8006/v1/health/ready
```

If the `status` field is `"ok"`, Xavier is ready to process RAG.

## 5. Web Panel (Dashboard)

Visit [http://localhost:8006/panel](http://localhost:8006/panel) to visualize your memories, knowledge graphs and system status in real time.
