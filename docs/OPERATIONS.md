# Xavier System Operations Runbook

This document details administration, deployment, configuration and troubleshooting tasks to keep the **Xavier** ecosystem operationally healthy. It covers the lifecycle of local Large Language Model (LLM) services (Ollama), automatic and manual transition to the fallback system (OpenRouter/OpenAI), token consumption optimization and the recovery runbook for outages.

---

## 📋 Table of Contents
1. [Bring Up and Verify Ollama (Local Models)](#1-bring-up-and-verify-ollama-local-models)
2. [Local Model vs. Cloud Fallback (OpenRouter)](#2-local-model-vs-cloud-fallback-openrouter)
3. [Heartbeat Optimization & Token Consumption](#3-heartbeat-optimization--token-consumption)
4. [Service Recovery Runbook (systemd)](#4-service-recovery-runbook-systemd)
5. [Daily Health Checklist](#5-daily-health-checklist)

---

## 1. Bring Up and Verify Ollama (Local Models)

Xavier's default local engine depends on **Ollama** running in the background. When Ollama is down, Xavier's `/health` endpoint will report the LLM as `unhealthy`.

### 1.1 Start and Verify the Ollama Process

#### On Linux (systemd)
```bash
# Check service status
systemctl status ollama

# Start service if stopped
sudo systemctl start ollama

# Stop service
sudo systemctl stop ollama

# Restart service
sudo systemctl restart ollama
```

#### On Windows or Mac (User Process)
If running as a desktop app, verify the process via CLI or Task Manager:
```bash
# Find active Ollama processes
pgrep -af ollama
```
If not running, start it from terminal or UI:
```bash
ollama serve > ollama_serve.log 2>&1 &
```

### 1.2 Test Ollama Endpoints Directly
Check if Ollama responds correctly on its standard port (`11434`):

```bash
# 1. Check general connectivity and version
curl http://localhost:11434/api/version

# 2. List downloaded local models
curl http://localhost:11434/api/tags
```

### 1.3 Manage Required Models
Xavier requires a language model (LLM) and a local embeddings model in its base configuration:
- **Default LLM:** `qwen3-coder` (or another compatible local model like `llama3`)
- **Recommended embedder:** `embeddinggemma`, `nomic-embed-text` or `mxbai-embed-large-v1`

#### Download models manually:
```bash
# Download code/text generation model
ollama pull qwen3-coder
# Download local embeddings model
ollama pull embeddinggemma
```

#### Test the Ollama Embeddings API:
To verify the local embeddings model works correctly before starting Xavier, run:
```bash
curl -X POST http://localhost:11434/api/embeddings \
  -H "Content-Type: application/json" \
  -d '{"model": "embeddinggemma", "prompt": "Xavier Local Embedder Verification Test"}'
```
*Expected response:* JSON with key `"embedding"` containing a 768-length float vector.

---

## 2. Local Model vs. Cloud Fallback (OpenRouter)

Xavier has a native fallback system that transparently switches between local execution (Ollama) and cloud processing (OpenRouter/OpenAI) if the local service is unresponsive or missing required models.

### 2.1 Configuration Environment Variables

| Environment Variable | Type / Value | Description |
|---------------------|--------------|-------------|
| `XAVIER_EMBEDDING_PROVIDER_MODE` | `local` \| `local-gllm` \| `cloud` \| `auto` \| `disabled` | Embedding provider mode. |
| `XAVIER_EMBEDDING_LOCAL_URL` | `http://localhost:11434/v1/embeddings` | Local embeddings endpoint URL (Ollama). |
| `XAVIER_EMBEDDING_URL` | `https://openrouter.ai/api/v1/embeddings` | Cloud embeddings provider URL (fallback). |
| `XAVIER_EMBEDDING_MODEL` | `embeddinggemma` \| `text-embedding-3-small` | Embeddings model to use by provider. |
| `XAVIER_OPENROUTER_API_KEY` | `sk-or-...` | Access token for OpenRouter API. |
| `OPENAI_API_KEY` | `sk-proj-...` | Access token for OpenAI (takes precedence over OpenRouter). |
| `XAVIER_MODEL_PROVIDER` | `local` \| `cloud` | Indicates primary provider for LLM. |
| `XAVIER_LOCAL_LLM_URL` | `http://localhost:11434/v1` | Base URL for local LLM API. |
| `XAVIER_LOCAL_LLM_MODEL` | `qwen3-coder` | Local language model configured for inference. |

### 2.2 Embeddings Fallback Flow (Auto-Fallback)

When `XAVIER_EMBEDDING_PROVIDER_MODE` is set to `auto` or left undefined, Xavier performs the following sequential logic:

1. **Ollama Reachability Probe:**
   Xavier sends a short HTTP request to `http://localhost:11434/v1/models` to verify Ollama connectivity.
2. **Model Check:**
   - If Ollama responds and has `embeddinggemma` installed, **Local** mode is activated.
   - If Ollama responds but **does not** have `embeddinggemma` installed, it will emit a warning (visible via `xavier doctor` and system alerts) but keep local execution intent.
3. **Cloud Fallback Activation:**
   - If Ollama is **not** reachable and environment credentials exist (`XAVIER_OPENROUTER_API_KEY` or `OPENAI_API_KEY`), Xavier automatically switches to the cloud provider.
   - It will use model **`text-embedding-3-small`** (flattened to 1536 dims) at the OpenRouter (`https://openrouter.ai/api/v1/embeddings`) or OpenAI endpoint transparently.
4. **No-Op Mode:**
   - If no local services are available and no environment keys are configured, the system will start in degraded mode with the `NoopEmbedder` encoder (dimension `0`).

### 2.3 Forcing Specific Operation Modes

#### Local-Only Configuration (No External Fallback):
```bash
export XAVIER_EMBEDDING_PROVIDER_MODE=local
export XAVIER_EMBEDDING_MODEL=embeddinggemma
export XAVIER_EMBEDDING_LOCAL_URL=http://localhost:11434/v1/embeddings
```

#### Cloud-Only Configuration (OpenRouter / OpenAI):
```bash
export XAVIER_EMBEDDING_PROVIDER_MODE=cloud
export XAVIER_EMBEDDING_MODEL=text-embedding-3-small
export XAVIER_EMBEDDING_URL=https://openrouter.ai/api/v1/embeddings
export XAVIER_OPENROUTER_API_KEY=«redacted:sk-…»
```

---

## 3. Heartbeat Optimization & Token Consumption

### 3.1 The Token Burn Problem in QwenCloud / OpenClaw
When Xavier is integrated with agents coordinated via external gateways (like QwenCloud or OpenClaw), agents send periodic "heartbeat" requests to keep the session and liveness state active.

By default, a background gateway heartbeat can run every **30 minutes**, consuming inference API calls (tokens) continuously even when the platform is idle. This causes accelerated drain of subscription plans and balance (Token Plan).

### 3.2 Tuning Recommendations (Ops Runbook)
To mitigate this token leakage in production, it is recommended to configure the agent heartbeat interval to a wider spectrum or disable the gateway during scheduled idle periods.

#### Adjust Heartbeat Configuration:
Look for the `heartbeat` or `agents.defaults.heartbeat.every` property in the global configuration file (`config/xavier.config.json` or `.openclaw` for your sub-agents) and modify it:

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
*Raising the value from `30m` to `1h` or `2h` reduces passive token consumption by 50% to 75%.*

#### Stop the Gateway When Not in Use:
If you are off-hours, stop the agent bridge to avoid token plan drain:
```bash
# Stop OpenClaw/QwenCloud gateway
# (Depending on your agent deployment)
pkill -f openclaw-gateway
```

---

## 4. Service Recovery Runbook (systemd)

If Xavier stops responding on the central API HTTP port (default `3000` or `XAVIER_MCP_PORT`), follow this structured runbook to return the system to a healthy state.

### Step 1: Check systemd Service Status
```bash
# Get detailed xavier service status
systemctl status xavier

# Check last service logs
journalctl -u xavier -n 100 --no-pager
```

### Step 2: Service Is "Active" but Not Responding (Blocked Socket)
If `systemctl` reports the service as active but there is no API response, the port is likely blocked by an orphan process or hung thread.

```bash
# 1. Check what process is listening on port 3000
lsof -i :3000

# 2. Find Xavier Process ID (PID)
pgrep -af xavier
```

### Step 3: Forced Stop and Cleanup Procedure
If the service does not respond to `systemctl stop xavier`, force-terminate the subprocess:

```bash
# Stop service at system level
sudo systemctl stop xavier 2>/dev/null || true
# Kill any residual xavier binary processes
sudo kill -9 $(pgrep -f xavier) 2>/dev/null || true

# Free TCP socket if still in TIME_WAIT or busy
sudo kill -9 $(lsof -t -i :3000) 2>/dev/null || true
```

### Step 4: Bring Up and Verify
```bash
# Start service again
sudo systemctl start xavier

# Monitor startup in real time
journalctl -u xavier -f
```

### Step 5: Manual Standalone Fallback
If the systemd environment is corrupted or has permission failures, you can bring Xavier up in manual mode isolated in background:

```bash
# Ensure required environment variables
export XAVIER_DATA_DIR="/opt/xavier/data"
export XAVIER_EMBEDDING_PROVIDER_MODE="auto"

# Run binary independently saving logs
nohup ./xavier --mcp-port 0 > /var/log/xavier_standalone.log 2>&1 &

# Check it is running
ps aux | grep xavier
```

---

## 5. Daily Health Checklist

The system administrator should run this health checklist at the start of each day to ensure resilience and optimal performance.

### ▢ 1. Run Local Diagnostic Command (`xavier doctor`)
The integrated utility performs automatic audits of database, LLM accessibility and embedding consistency.
```bash
xavier doctor --verbose
```
*Validation:* All crucial output rows must show **`[✓] OK`**. If an embedding mismatch is detected (`Embedding Model Consistency` at `WARN`), plan a memory re-index or adjust `XAVIER_EMBEDDING_MODEL`.

### ▢ 2. Verify Code Index Status (CodeGraph)
```bash
xavier code status
```
*Validation:* Confirm that the number of indexed files matches the main working branch and that the CodeGraph database file (`data/code_graph.db`) is not corrupt or empty (`total_symbols > 0`).

### ▢ 3. Monitor SQLite Database Disk Usage
Production SQLite databases accumulate records and fragmentation.
```bash
# Check memory vector database size
ls -lh /opt/xavier/data/xavier_memory_vec.db

# Check security audit database
ls -lh /opt/xavier/data/security.db
```
*Preventive maintenance:* If size is excessive, run manual compaction or the nightly consolidation process:
```bash
# Perform compaction and purge of Xavier nightly expirations
xavier memory consolidate
```

### ▢ 4. Verify Port and HTTP Endpoint Availability
```bash
# Validate general health endpoint
curl -s http://localhost:3000/health | jq .

# Validate readiness endpoint
curl -s http://localhost:3000/readiness | jq .

# Validate Mesh dashboard (peer-to-peer communication and latencies)
curl -s -H "Authorization: Bearer <YOUR_TOKEN>" http://localhost:3000/v1/mesh/health | jq .
```

### ▢ 5. Inspect RAM Usage of Ollama and Xavier
```bash
free -m
ps -eo pid,ppid,cmd,%mem,%cpu --sort=-%mem | head -n 10
```
*Validation:* Ensure GPU acceleration layers are not overloading host memory causing the kernel to invoke the Out-Of-Memory Killer (`OOM-killer`).
