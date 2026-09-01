# Xavier Local-First Guide (Ollama)

This guide explains how to configure Xavier to run 100% locally using [Ollama](https://ollama.com/), after the improvements introduced in Wave 2.

Xavier now implements a native local-first architecture where the panel chat automatically uses the local LLM and embeddings, handling redundancy and degraded mode gracefully.

---

## 🚀 Post-Wave 2: 100% Local Operation (Quickstart)

The quickstart to get Xavier running offline and locally involves installing Ollama, downloading the optimized models and starting the service.

### Prerequisites

*   **Ollama**: Download and install Ollama from [ollama.com](https://ollama.com/).
*   **Running Service**: Ensure the Ollama daemon is running (`ollama serve`). You can verify by opening `http://localhost:11434` in your browser or via curl.

### 3-Step Setup

Run the following commands in your terminal to prepare models and start Xavier:

1.  **Download the local LLM for chat and reasoning:**
    ```bash
    ollama pull qwen3-coder
    ```

2.  **Download the local model for semantic embeddings:**
    ```bash
    ollama pull embeddinggemma
    ```

3.  **Start the Xavier server:**
    ```bash
    xavier serve
    ```
    *(Note: In a development environment, you can use `cargo run -- serve`)*

---

## 🐳 Docker (one command)

To simplify Xavier + Ollama deployment 100% locally, you can use Docker Compose. This lets you bring up Xavier and Ollama together with pre-downloaded models in a single command.

### Prerequisites

*   **Docker** and **Docker Compose** (V2 recommended) installed.

### Deployment Instructions

1.  **Copy the example configuration:**
    Create your `.env` file from the Docker template:
    ```bash
    cp docker/.env.docker.example docker/.env
    ```
    *(Note: Be sure to edit `docker/.env` to set your `XAVIER_TOKEN` or change options like `XAVIER_LOG_LEVEL` if needed).*

2.  **Download local models (first time only):**
    Use the `init` profile to bring up Ollama and automatically download the required LLM (`qwen3-coder`) and embeddings (`embeddinggemma`) models:
    ```bash
    docker compose -f docker/docker-compose.local.yml --env-file docker/.env --profile init up --build
    ```
    This will build the Xavier image, start Ollama, wait until healthy and then the `ollama-init` container will download models directly into the shared persistent volume. Once done, the init container stops.

3.  **Start the full environment in background:**
    To start Xavier and Ollama ready for production:
    ```bash
    docker compose -f docker/docker-compose.local.yml --env-file docker/.env up -d
    ```
    Xavier will be available at `http://localhost:8006`, communicating natively with the `ollama` service inside the Docker network.

### GPU Support (Optional)

If you have an NVIDIA GPU and the **NVIDIA Container Toolkit** installed, you can accelerate inference by uncommenting the GPU resources section in `docker/docker-compose.local.yml`:

```yaml
    # To enable NVIDIA GPU support, uncomment:
    deploy: resources: reservations: devices: [{driver: nvidia, capabilities: [gpu]}]
```

---

## 🔍 System Verification

### 1. Boot Log (Console)
On startup, Xavier scans system capabilities (including Ollama and its models). The console init log should confirm correct operation with a banner like:

```text
🟢 Xavier started — mode: LOCAL
   LLM:        ollama/qwen3-coder @ http://localhost:11434/v1 [reachable]
   Embeddings: ollama/embeddinggemma @ localhost:11434 [reachable]
   Vector DB:  sqlite_vec (vec-store.sqlite3)
```

This log indicates both local services are `[reachable]` and ready to process inference.

### 2. Panel UI Badge
Once the server is started, open the graphical interface. In the provider selector or system status, you should see:

*   **Badge**: `🦙 Local` (green, indicating healthy local inference).

---

## 🔄 Resilience & Automatic Fallback

Xavier is designed not to interrupt user workflow on provider outages.

1.  **Mixed-Priority Fallback Chain:**
    If you configure both cloud providers (like OpenAI or Anthropic) and local, the internal fallback chain evaluates cloud first and local as last resort (or vice versa if you force strict local mode).
2.  **Transparent Transition:**
    If the primary configured provider (e.g. OpenAI) experiences an outage or quota exhaustion, Xavier automatically and transparently redirects the chat request to the local Ollama backend (`qwen3-coder`).

---

## 💾 Graceful Degradation (Degraded Mode)

What happens if even Ollama or the local hardware fails? Xavier implements elegant degradation to avoid blank responses or hangs:

1.  **Transition to Local Degraded:**
    If Ollama endpoints stop responding after several retries, the health monitor (`HealthMonitor`) switches the operational state to `local-degraded`.
2.  **UI Visual Badge:**
    The panel indicator updates to show:
    *   **Badge**: `⚠️ Degraded` (yellow, alerting local inference engine unavailability).
3.  **Memory-based Responses:**
    In this state, chat will not fail with connection errors. Instead, the orchestrator activates deep memory fallback. It generates a contextualized response directly from hot documents, episodic summaries and engrams in the local vector DB (`sqlite-vec`), accompanied by a clear visual indicator:
    *   **UI Note**: `💾` (indicator that the response was recovered from the offline persistent memory database).

---

## ⚙️ Configuration Reference (Environment Variables)

Xavier configuration is managed via environment variables (defined in your `.env` file). Ensure they match the local configuration spec exactly:

```env
# Primary inference provider (values: local, cloud, opencode, etc.)
XAVIER_MODEL_PROVIDER=local

# Local LLM endpoint URL (Ollama exposes OpenAI-compatible API at /v1)
XAVIER_LOCAL_LLM_URL=http://localhost:11434/v1

# Exact language model name downloaded in Ollama
XAVIER_LOCAL_LLM_MODEL=qwen3-coder

# Embedding provider mode (values: local for Ollama, local-gllm for native Candle, cloud)
XAVIER_EMBEDDING_PROVIDER_MODE=local

# Ollama embeddings endpoint URL
XAVIER_EMBEDDING_URL=http://localhost:11434/api/embeddings

# Exact embeddings model name downloaded in Ollama
XAVIER_EMBEDDING_MODEL=embeddinggemma
```

---

## 🛠️ Troubleshooting

### Validate Ollama Status via API
If you suspect Ollama is not responding, run the following in your terminal to list loaded local models:
```bash
curl http://localhost:11434/api/tags
```
You should receive a JSON response containing `qwen3-coder` and `embeddinggemma`.

### UI Badge Shows `⚠️ Degraded`
1.  Verify Ollama is running in background (`lsof -i :11434` or `Get-Process ollama` on Windows).
2.  Ensure no port conflict from another instance or database.
3.  Confirm you downloaded the exact model names corresponding to your `.env` variables.

### Hot-Swapping Provider
You can force Xavier inference provider switch at any time in two ways:

1.  **Via Xavier CLI:**
    ```bash
    xavier provider set local
    ```
2.  **Via HTTP API Endpoint:**
    Send a POST request to the Xavier server to change the active provider:
    ```bash
    curl -X POST http://localhost:8006/v1/provider/set \
      -H "Content-Type: application/json" \
      -H "X-Xavier-Token: YOUR_AUTH_TOKEN" \
      -d '{"provider": "local"}'
    ```

---

## 🔗 Related Links
*   **Local Embeddings:** For details on local embeddings, comparison between Ollama and native GLLM (Candle) mode, see the [Local Embeddings Guide](LOCAL_EMBEDDINGS.md).
*   **Local LLM Bridges:** If you want to use alternatives like LM Studio or the opencode CLI bridge, see the [LLM Bridges Guide](LOCAL_LLM_BRIDGES.md).
*   **Development Roadmap:** Explore the long-term vision for offline infrastructure in the [Local-First Roadmap](ROADMAP_LOCAL_FIRST.md).
