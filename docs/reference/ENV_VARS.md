# Environment Variables Reference

<!-- BEGIN AUTOGEN -->

## CLI / Legacy

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_API_URL` | `string` | (falls back to `XAVIER_URL`) | CLI client URL (legacy) | ❌ |
| `XAVIER_AUTH_TOKEN` | `string` | (falls back to `XAVIER_TOKEN`) | CLI client auth token (legacy) | ✅ |

## Context Budget

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_CTX_PC_MAX_DOCS` | `usize` | `10` | Maximum docs for precompact | ❌ |
| `XAVIER_CTX_PC_MAX_TOKENS` | `usize` | `3200` | Maximum tokens for precompact | ✅ |
| `XAVIER_CTX_PC_MED_DOCS` | `usize` | `7` | Medium docs for precompact | ❌ |
| `XAVIER_CTX_PC_MED_TOKENS` | `usize` | `1600` | Medium tokens for precompact | ✅ |
| `XAVIER_CTX_PC_MIN_DOCS` | `usize` | `4` | Minimal docs for precompact | ❌ |
| `XAVIER_CTX_PC_MIN_TOKENS` | `usize` | `800` | Minimal tokens for precompact | ✅ |
| `XAVIER_CTX_SS_MAX_DOCS` | `usize` | `8` | Maximum docs at session start | ❌ |
| `XAVIER_CTX_SS_MAX_TOKENS` | `usize` | `2400` | Maximum tokens at session start | ✅ |
| `XAVIER_CTX_SS_MED_DOCS` | `usize` | `5` | Medium docs at session start | ❌ |
| `XAVIER_CTX_SS_MED_TOKENS` | `usize` | `1200` | Medium tokens at session start | ✅ |
| `XAVIER_CTX_SS_MIN_DOCS` | `usize` | `3` | Minimal docs at session start | ❌ |
| `XAVIER_CTX_SS_MIN_TOKENS` | `usize` | `600` | Minimal tokens at session start | ✅ |

## Embedding

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_EMBEDDER` | `string` | `—` | Legacy embedder selection | ❌ |
| `XAVIER_EMBEDDING_API_FLAVOR` | `string` | `"openai-compatible"` | API flavor: `openai-compatible`, `anthropic-compatible` | ❌ |
| `XAVIER_EMBEDDING_API_KEY` | `string` | (none) | API key for embedding provider | ✅ |
| `XAVIER_EMBEDDING_ENDPOINT` | `string` | (same as EMBEDDING_URL) | Alternative embedding endpoint (takes precedence if set) | ❌ |
| `XAVIER_EMBEDDING_MODEL` | `string` | `"embeddinggemma"` | Embedding model name | ❌ |
| `XAVIER_EMBEDDING_URL` | `string` | `"http://localhost:11434/v1"` | Primary embedding endpoint URL | ❌ |
| `XAVIER_OPENROUTER_API_KEY` | `string` | (none) | OpenRouter-specific API key for embeddings | ✅ |

## LLM Provider

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_API_FLAVOR` | `string` | `"openai-compatible"` | API protocol: `openai-compatible`, `anthropic-compatible` | ❌ |
| `XAVIER_CLOUD_LLM_MODEL` | `string` | `_none (required)_` | Cloud LLM model (generic mode) | ❌ |
| `XAVIER_CLOUD_LLM_URL` | `string` | `"https://api.openai.com/v1"` | Cloud LLM URL (generic mode) | ❌ |
| `XAVIER_LLM_API_KEY` | `string` | (none) | Generic LLM API key (fallback for all providers) | ✅ |
| `XAVIER_LLM_MODEL` | `string` | `_none (required)_` | Fallback model name for all providers | ❌ |
| `XAVIER_LOCAL_ANTHROPIC_URL` | `string` | `"http://localhost:11434"` | Local Anthropic-compatible endpoint | ❌ |
| `XAVIER_LOCAL_LLM_API_KEY` | `string` | `"ollama"` | Local LLM API key (defaults to "ollama") | ✅ |
| `XAVIER_LOCAL_LLM_MODEL` | `string` | `_none (required)_` | Local LLM model name (example only: `<any model your server lists at /v1/models>`) | ❌ |
| `XAVIER_LOCAL_LLM_URL` | `string` | `"http://localhost:11434/v1"` | Local LLM endpoint (Ollama, vLLM, llama.cpp, etc.) | ❌ |
| `XAVIER_MODEL_PROVIDER` | `string` | `"local"` | LLM provider: `local`, `cloud`, `disabled`, `openrouter`, `groq`, `anthropic`, `openai`, `deepseek`, `minimax`, `gemini` | ❌ |

## Maintenance Jobs

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_GC_DESTRUCTIVE` | `bool` | `—` | Opt-in scheduled maintenance job for garbage collection (destructive) | ❌ |
| `XAVIER_SEMANTIC_COMPACTION` | `bool` | `—` | Opt-in scheduled maintenance job for semantic compaction | ❌ |
| `XAVIER_TGD_SCHEDULER` | `bool` | `—` | Opt-in scheduled nightly TGD consolidation | ❌ |

## Memory Backend

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_DATA_DIR` | `string` | `"data"` | Base data directory | ❌ |
| `XAVIER_EMBEDDING_DIMENSIONS` | `usize` | `768` | Embedding vector dimensions | ❌ |
| `XAVIER_MEMORY_BACKEND` | `string` | `"vec"` | Storage backend: `vec`, `sqlite`, `file` | ❌ |
| `XAVIER_MEMORY_FILE_PATH` | `string` | `"data/workspaces/default/memory-store.json"` | JSON file store path | ❌ |
| `XAVIER_MEMORY_SQLITE_PATH` | `string` | `"data/memory-store.sqlite3"` | SQLite store path | ❌ |
| `XAVIER_MEMORY_VEC_PATH` | `string` | `"data/vec-store.sqlite3"` | Vector SQLite store path | ❌ |
| `XAVIER_WORKSPACE_DIR` | `string` | `"data/workspaces"` | Workspace data directory | ❌ |

## Memory Layers

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_EPISODIC_MIN_EVENT_IMPORTANCE` | `f32` | `0.5` | Minimum importance score for key events | ❌ |
| `XAVIER_EPISODIC_SUMMARY_WINDOW` | `usize` | `10` | Turns before triggering episodic summary | ❌ |
| `XAVIER_MAX_EPISODIC_SESSIONS` | `usize` | `50` | Maximum sessions retained in episodic memory | ❌ |
| `XAVIER_WORKING_BM25_B` | `f32` | `0.75` | BM25 scoring parameter b | ❌ |
| `XAVIER_WORKING_BM25_K1` | `f32` | `1.5` | BM25 scoring parameter k1 | ❌ |
| `XAVIER_WORKING_LRU_THRESHOLD` | `usize` | `2` | Access count threshold for LRU exemption | ❌ |
| `XAVIER_WORKING_MEMORY_CAPACITY` | `usize` | `100` | Maximum items in working memory | ❌ |

## Retrieval

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_DISABLE_HYDE` | `bool` | `true` | Disable HyDE (Hypothetical Document Embedding) query expansion | ❌ |

## Routing

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_EXPERT_KEEP_ALIVE` | `string` | `"5m"` | Expert router keep-alive timeout | ❌ |
| `XAVIER_EXPERT_THRESHOLD` | `f32` | `0.6` | Expert router relevance threshold | ❌ |
| `XAVIER_ROUTER_COMPLEX_MODEL` | `string` |  | Model for complex reasoning queries | ❌ |
| `XAVIER_ROUTER_FAST_MODEL` | `string` | (none) | Model for fast/direct queries (router override) | ❌ |
| `XAVIER_ROUTER_POLICY_PATH` | `string` | (none) | Path to JSON routing policy file | ❌ |
| `XAVIER_ROUTER_POLICY_REFRESH_SECS` | `u64` | `30` | Policy file refresh interval | ❌ |
| `XAVIER_ROUTER_QUALITY_MODEL` | `string` | (none) | Model for quality queries (router override) | ❌ |
| `XAVIER_ROUTER_RETRIEVED_MODEL` | `string` |  | Model for retrieved-augmented queries | ❌ |

## Server & Network

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_ALLOWED_DOMAINS` | `string` | (none) | Comma-separated domain allowlist for URL validator (test-only) | ❌ |
| `XAVIER_ALLOW_CLI_TOKEN` | `bool` | `false` | Gates the `--token` CLI flag for node provisioning (`src/nodes/provision.rs`): only honored when `1`/`true`. Test/dev only — production must use `XAVIER_NODE_TOKEN` or stdin. | ✅ |
| `XAVIER_AUTH_RATE_LIMIT` | `u32` | `20` | Max requests per minute, per caller, accepted by the whole `/auth/*` nest (register/login/refresh/logout/2FA/recovery/OAuth), sliding 60s window. See `src/cli/http_setup.rs::auth_rate_limit_middleware`. | ❌ |
| `XAVIER_CODE_GRAPH_DB_PATH` | `string` | `"data/code_graph.db"` | Path to code graph SQLite DB | ❌ |
| `XAVIER_CONFIG_PATH` | `string` | `"config/xavier.config.json"` | Override config file location | ❌ |
| `XAVIER_HOST` | `string` | `"127.0.0.1"` | IP address to bind the HTTP server | ❌ |
| `XAVIER_JWT_SECRET` | `string` | (none) | HMAC signing secret for user-session JWTs issued by `/auth/login` and validated by the MCP/API JWT middleware. Must be set for `/auth/*` login to work at all. | ✅ |
| `XAVIER_LOG_LEVEL` | `string` | `"info"` | Log level: `trace`, `debug`, `info`, `warn`, `error` | ❌ |
| `XAVIER_MCP_PORT` | `u16` | `crate::cli::mcp::DEFAULT_MCP_PORT` | Port for the MCP HTTP+SSE server started alongside `xavier http`. `0` disables the MCP HTTP endpoint. | ❌ |
| `XAVIER_PANEL_STORE_DIR` | `string` | (see `XAVIER_PANEL_UI_DIR` resolution) | Override directory for the Maloca/panel conversations store. | ❌ |
| `XAVIER_PORT` | `u16` | `8006` | HTTP server port | ❌ |
| `XAVIER_SECRET_EGRESS` | `string` | `—` | Egress policy configuration | ❌ |
| `XAVIER_SHUTDOWN_GRACE_SECS` | `u64` | `10` | Server shutdown grace period in seconds | ❌ |
| `XAVIER_STATE_DIR` | `string` | `$HOME` (then `$USERPROFILE`, then `.`)` | Base directory for persistent server state: `.xavier/auth.db`, Maloca store, node registry, etc. Set it explicitly (e.g. `/data` in Docker) so state survives container restarts. | ❌ |
| `XAVIER_TLS_CERT` / `XAVIER_TLS_KEY` | `string` (PEM paths)` | (none) | When BOTH are set, the HTTP server terminates TLS 1.3 itself (`axum_server::bind_rustls`) instead of plain HTTP. Either var alone is ignored (both required). | ✅ |
| `XAVIER_TOKEN` | `string` | `—` | Authentication token for HTTP API and CLI clients | ✅ |
| `XAVIER_TOKEN_SECRET` | `string` | `—` | Secret key for HMAC signing in sync/verification | ✅ |
| `XAVIER_URL` | `string` | `"http://localhost:8006"` | Full URL for client, verification, and sync connections | ❌ |

## Synchronization

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_SYNC_INTERVAL_MS` | `u64` | `300000` | Interval between sync cycles (ms) | ❌ |
| `XAVIER_SYNC_LAG_THRESHOLD_MS` | `u64` | `30000` | Maximum allowed write lag (ms) | ❌ |
| `XAVIER_SYNC_MAX_RETRIES` | `u32` | `3` | Maximum health check retries | ❌ |
| `XAVIER_SYNC_MIN_HEALTH_INTERVAL_MS` | `u64` | `1000` | Min interval between health checks | ❌ |
| `XAVIER_SYNC_RETRY_DELAY_MS` | `u64` | `1000` | Delay between retries (ms) | ❌ |
| `XAVIER_SYNC_SAVE_OK_RATE_THRESHOLD` | `f32` | `0.95` | Minimum save success rate | ❌ |
| `XAVIER_SYNC_TIMEOUT_MS` | `u64` | `5000` | Timeout per health check attempt | ❌ |

## Telegram Bot

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_TELEGRAM_ENABLED` | `bool` | `❌` | Enable Telegram bot integration | ❌ |
| `XAVIER_TELEGRAM_TOKEN` | `string` | `✅` | Telegram Bot API token | ✅ |

## Testing (internal)

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_CONFIG_SOURCE_CHILD` | `bool` | (unset) | Test-only marker for the config-source child-process test; not read in production | ❌ |
| `XAVIER_CONTRACT_LITERAL` | `string` | (unset) | Test-only value used to verify env-contract values are loaded literally; not read in production | ❌ |

## Training Jobs API

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_PYTHON` | `string` | `python3` | Interpreter used to run the trainer script | ❌ |
| `XAVIER_TRAIN_ARTIFACTS_BUDGET_BYTES` | `u64` | `21474836480` (20 GiB)` | Total size cap for job artifacts; over budget, failed/cancelled jobs are pruned first, then the oldest unreferenced succeeded jobs. `0` disables | ❌ |
| `XAVIER_TRAIN_MAX_ARTIFACT_BYTES` | `u64` | `4294967296` | Max GGUF upload size for manual notebook jobs | ❌ |
| `XAVIER_TRAIN_MAX_CONCURRENT` | `usize` | `1` | Max simultaneous queued/running local jobs (HTTP 429 beyond) | ❌ |
| `XAVIER_TRAIN_RETENTION_DAYS` | `u64` (days)` | `30` | Terminal jobs older than this lose `out/`, `artifacts/`, `notebook/` (logs and DB row stay). Artifacts referenced by a non-retired mini-expert are never deleted. `0` disables | ❌ |
| `XAVIER_TRAIN_SCRIPT` | `path` | `scripts/training/train_expert.py` (resolved against the install root)` | Trainer script | ❌ |
| `XAVIER_TRAIN_TIMEOUT` | `u64` (seconds)` | `21600` | Hard per-job wall-clock limit; the whole process group is killed. `0` disables | ❌ |

## Uncategorized

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_AGENTS_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_AGENT_REGISTRY` | `string` |  | Internal | ❌ |
| `XAVIER_AGENT_SCANNER_INTERVAL` | `string` |  | Internal | ❌ |
| `XAVIER_API_KEY` | `string` |  | Internal | ✅ |
| `XAVIER_AUTH_VAULT_SERVICE` | `string` |  | Internal | ❌ |
| `XAVIER_AUTO_SAVE_TIMEOUT_MS` | `string` |  | Internal | ❌ |
| `XAVIER_AUTO_VERIFY` | `string` |  | Internal | ❌ |
| `XAVIER_BENCHMARK_DATASET` | `string` |  | Internal | ❌ |
| `XAVIER_BILLING_URL` | `string` |  | Internal | ❌ |
| `XAVIER_BULK_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_BUS_QUOTA_PER_WINDOW` | `string` |  | Internal | ❌ |
| `XAVIER_BUS_QUOTA_WINDOW_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_CB_FAILURES` | `string` |  | Internal | ❌ |
| `XAVIER_CB_OPEN_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_CHRONICLE_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_CLEARANCE_AUDIT_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_CLEARANCE_AUDIT_SQLITE` | `string` |  | Internal | ❌ |
| `XAVIER_CLEARANCE_TRUST_HEADER` | `string` |  | Internal | ❌ |
| `XAVIER_CLOUD` | `string` |  | Internal | ❌ |
| `XAVIER_CLOUD_BACKUP_INTERVAL_MINS` | `string` |  | Internal | ❌ |
| `XAVIER_CLOUD_BACKUP_PASSPHRASE` | `string` |  | Internal | ✅ |
| `XAVIER_CLOUD_BACKUP_PASSPHRASE_FILE` | `string` |  | Internal | ✅ |
| `XAVIER_CODEGRAPH_BIN` | `string` |  | Internal | ❌ |
| `XAVIER_CODEGRAPH_INSTALL_ACTIVE` | `string` |  | Internal | ❌ |
| `XAVIER_CODEGRAPH_REPROMPT` | `string` |  | Internal | ❌ |
| `XAVIER_CODEGRAPH_UDS_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_CODE_EXTRA_ROOTS` | `string` |  | Internal | ❌ |
| `XAVIER_CODE_GRAPH_SKIP_EMBED` | `string` |  | Internal | ❌ |
| `XAVIER_CODE_SCAN` | `string` |  | Internal | ❌ |
| `XAVIER_CODE_SYNC_INTERVAL_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_COMMAND_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_CONFIG_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_CONTEXT_SNAPSHOTS` | `string` |  | Internal | ❌ |
| `XAVIER_CRON_SLEEP_MINUTES` | `string` |  | Internal | ❌ |
| `XAVIER_DATASETS_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_DEFAULT_CLEARANCE` | `string` |  | Internal | ❌ |
| `XAVIER_DEFAULT_KEY_TTL_SECS` | `string` |  | Internal | ✅ |
| `XAVIER_DRIVE_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_EMAIL_DEDUP_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDER_CB_COOLDOWN_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDER_CB_THRESHOLD` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_CAPACITY` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_DB_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_PERSIST` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_SIZE` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_TTL` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CACHE_TTL_HOURS` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_CLOUD_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_FALLBACK_BUDGET_MS` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_LOCAL_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_LOCAL_URL` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_PROVIDER` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_TIMEOUT_MS` | `string` |  | Internal | ❌ |
| `XAVIER_EMBEDDING_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_EMBED_PROVIDER` | `string` |  | Internal | ❌ |
| `XAVIER_EMBED_WARMUP` | `string` |  | Internal | ❌ |
| `XAVIER_EMBED_WARMUP_INTERVAL_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_ENCRYPTION_AT_REST_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_ENTERPRISE_DB_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_ENV_FILE` | `string` |  | Internal | ❌ |
| `XAVIER_EPHEMERAL_MAX_TOKENS` | `string` |  | Internal | ✅ |
| `XAVIER_EPISODIC_LLM_SUMMARY_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_ERROR_INTERNAL` | `string` |  | Internal | ❌ |
| `XAVIER_ERROR_NOT_FOUND` | `string` |  | Internal | ❌ |
| `XAVIER_ERROR_SECURITY` | `string` |  | Internal | ❌ |
| `XAVIER_ERROR_VALIDATION` | `string` |  | Internal | ❌ |
| `XAVIER_EVENT_LOG_MAX_AGE_DAYS` | `string` |  | Internal | ❌ |
| `XAVIER_EVENT_LOG_MAX_ROWS` | `string` |  | Internal | ❌ |
| `XAVIER_EXEC_CANARY` | `string` |  | Internal | ❌ |
| `XAVIER_EXEC_HANDLER_CANARY` | `string` |  | Internal | ❌ |
| `XAVIER_EXPERT_FALLBACK_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_EXPLAIN_API_KEY_FILE` | `string` |  | Internal | ✅ |
| `XAVIER_EXPLAIN_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_EXPLAIN_PROVIDER` | `string` |  | Internal | ❌ |
| `XAVIER_FAILED_SYNCS_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_FILE_STORE_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_GLLM_DIMENSION` | `string` |  | Internal | ❌ |
| `XAVIER_GLLM_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_GLLM_MODEL_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_GOOGLE_CLIENT_ID` | `string` |  | Internal | ❌ |
| `XAVIER_GOOGLE_CLIENT_SECRET` | `string` |  | Internal | ✅ |
| `XAVIER_GUARD_REAL_HOME` | `string` |  | Internal | ❌ |
| `XAVIER_HC_SESSIONS_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_HEADLESS` | `string` |  | Internal | ❌ |
| `XAVIER_HOME` | `string` |  | Internal | ❌ |
| `XAVIER_HTTP_HOST` | `string` |  | Internal | ❌ |
| `XAVIER_HTTP_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_HYDE_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_IMPROVE_STAGE_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_IMPROVE_TOTAL_TIMEOUT_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_INGESTION_INTERVAL_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_INSIDE_BUILD` | `string` |  | Internal | ❌ |
| `XAVIER_INSIDE_TEST` | `string` |  | Internal | ❌ |
| `XAVIER_INSIDE_TEST_INIT` | `string` |  | Internal | ❌ |
| `XAVIER_ISOLATE_INIT` | `string` |  | Internal | ❌ |
| `XAVIER_IS_MAINTAINER` | `string` |  | Internal | ❌ |
| `XAVIER_KEYWORD_WEIGHT` | `string` |  | Internal | ✅ |
| `XAVIER_KEY_PREFIX` | `string` |  | Internal | ✅ |
| `XAVIER_LICENSE_PUBKEY` | `string` |  | Internal | ✅ |
| `XAVIER_LOG` | `string` |  | Internal | ❌ |
| `XAVIER_LOG_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_LOG_RETENTION_DAYS` | `string` |  | Internal | ❌ |
| `XAVIER_MAINTAINER_PRIVATE_KEY_HEX` | `string` |  | Internal | ✅ |
| `XAVIER_MARKET_ACCESS_GRANT_KEY_WRAP_V1` | `string` |  | Internal | ✅ |
| `XAVIER_MARKET_ANCHOR_INTERVAL_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_CURRENCY` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_IVN_QUORUM_RATIO` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_IVN_VALIDATORS_PER_REQUEST` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_LEGAL_TEXT_STATUS` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_LICENSES` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_SETTLEMENT` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_V2` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_V2_DB_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_MARKET_V2_ISSUER_SEED` | `string` |  | Internal | ❌ |
| `XAVIER_MASTER_KEY_NAME` | `string` |  | Internal | ✅ |
| `XAVIER_MAX_CONTEXT_TOKENS` | `string` |  | Internal | ✅ |
| `XAVIER_MEMORY_EAGER_LOAD` | `string` |  | Internal | ❌ |
| `XAVIER_MEMORY_FALLBACK` | `string` |  | Internal | ❌ |
| `XAVIER_MEMORY_WRITE_RPM` | `string` |  | Internal | ❌ |
| `XAVIER_MESH_AUTO_REPAIR` | `string` |  | Internal | ❌ |
| `XAVIER_MESH_FALLBACK` | `string` |  | Internal | ❌ |
| `XAVIER_MESH_LOCALHOST_ONLY` | `string` |  | Internal | ❌ |
| `XAVIER_MESH_SYNC_WORKSPACES` | `string` |  | Internal | ❌ |
| `XAVIER_MESH_TOKEN` | `string` |  | Internal | ✅ |
| `XAVIER_MIN_MATCH_SCORE` | `string` |  | Internal | ❌ |
| `XAVIER_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_NAME` | `string` |  | Internal | ❌ |
| `XAVIER_NAT_PUNCH_V1` | `string` |  | Internal | ❌ |
| `XAVIER_NODE_ID` | `string` |  | Internal | ❌ |
| `XAVIER_NODE_REGISTRY_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_NODE_TOKEN` | `string` |  | Internal | ✅ |
| `XAVIER_OAUTH_REDIRECT_BASE` | `string` |  | Internal | ❌ |
| `XAVIER_OAUTH_STATE_SECRET` | `string` |  | Internal | ✅ |
| `XAVIER_OLLAMA_DIMS` | `string` |  | Internal | ❌ |
| `XAVIER_OLLAMA_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_OLLAMA_URL` | `string` |  | Internal | ❌ |
| `XAVIER_OPENCODE_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_OPENROUTER_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_OPENROUTER_URL` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_ARM_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_ARM_WEIGHT` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_DB` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_DUMP` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_INGEST_ROOTS` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_LINES_PER_PAGE` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_MAX_NODE_TOKENS` | `string` |  | Internal | ✅ |
| `XAVIER_PAGEINDEX_MAX_RESPONSE_CHARS` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_MIN_NODE_TOKENS` | `string` |  | Internal | ✅ |
| `XAVIER_PAGEINDEX_PDFIUM_LIB` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_PDF_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_PDF_ONLY` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_SUMMARIZE` | `string` |  | Internal | ❌ |
| `XAVIER_PAGEINDEX_SUMMARY_MAX_WORDS` | `string` |  | Internal | ❌ |
| `XAVIER_PANEL_UI_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_PANEL_URL` | `string` |  | Internal | ❌ |
| `XAVIER_PEERS` | `string` |  | Internal | ❌ |
| `XAVIER_PGHEART_TOKEN` | `string` |  | Internal | ✅ |
| `XAVIER_PGHEART_URL` | `string` |  | Internal | ❌ |
| `XAVIER_PLUGINS_INDEX` | `string` |  | Internal | ❌ |
| `XAVIER_POSTGRES_URL` | `string` |  | Internal | ❌ |
| `XAVIER_POSTGRES_URL_2` | `string` |  | Internal | ❌ |
| `XAVIER_PRICE_EQUIPO` | `string` |  | Internal | ❌ |
| `XAVIER_PRICE_FUNDADOR` | `string` |  | Internal | ❌ |
| `XAVIER_PRICE_PRO` | `string` |  | Internal | ❌ |
| `XAVIER_PRICE_RESPALDO` | `string` |  | Internal | ❌ |
| `XAVIER_PROVIDER` | `string` |  | Internal | ❌ |
| `XAVIER_PROXY_RATE_LIMIT` | `string` |  | Internal | ❌ |
| `XAVIER_RECORD_KEY` | `string` |  | Internal | ✅ |
| `XAVIER_RECOVERY_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_REPO_MEMORY_DB_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_REPO_STORE_ROOTS` | `string` |  | Internal | ❌ |
| `XAVIER_REQUIRED_CLEARANCE_ROUTES` | `string` |  | Internal | ❌ |
| `XAVIER_RERANK_API_KEY` | `string` |  | Internal | ✅ |
| `XAVIER_RERANK_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_RERANK_ENDPOINT` | `string` |  | Internal | ❌ |
| `XAVIER_RERANK_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_RETENTION_SWEEP_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_ROOT` | `string` |  | Internal | ❌ |
| `XAVIER_RUNTIME_STATE_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_SANDBOX_ROOT` | `string` |  | Internal | ❌ |
| `XAVIER_SCAN_INTERVAL_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_SEARCH_DEADLINE_MS` | `string` |  | Internal | ❌ |
| `XAVIER_SEARCH_EXPANSION` | `string` |  | Internal | ❌ |
| `XAVIER_SEARCH_EXPANSION_BUDGET_MS` | `string` |  | Internal | ❌ |
| `XAVIER_SECRET` | `string` |  | Internal | ✅ |
| `XAVIER_SECRET_FALLBACK` | `string` |  | Internal | ✅ |
| `XAVIER_SKILL_MANIFEST` | `string` |  | Internal | ❌ |
| `XAVIER_SKILL_STORE` | `string` |  | Internal | ❌ |
| `XAVIER_SPACES` | `string` |  | Internal | ❌ |
| `XAVIER_SPACE_PASSWORD` | `string` |  | Internal | ✅ |
| `XAVIER_SPACE_RECOVERY_CODE` | `string` |  | Internal | ❌ |
| `XAVIER_SPACE_RETENTION_DAYS` | `string` |  | Internal | ❌ |
| `XAVIER_SPACE_TOKEN_TTL_DAYS` | `string` |  | Internal | ✅ |
| `XAVIER_SPACE_WORKSPACE_CACHE_MAX` | `string` |  | Internal | ❌ |
| `XAVIER_STORE_BUDGET` | `string` |  | Internal | ❌ |
| `XAVIER_SUPABASE_KEY` | `string` |  | Internal | ✅ |
| `XAVIER_SUPABASE_KEY_2` | `string` |  | Internal | ✅ |
| `XAVIER_SUPABASE_URL` | `string` |  | Internal | ❌ |
| `XAVIER_SUPABASE_URL_2` | `string` |  | Internal | ❌ |
| `XAVIER_SWAL_DIR` | `string` |  | Internal | ❌ |
| `XAVIER_SWAL_WORKSPACE` | `string` |  | Internal | ❌ |
| `XAVIER_SYNC_CLEARANCE_CEILING` | `string` |  | Internal | ❌ |
| `XAVIER_SYSTEMD_EMBEDDING_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_TELECOM_SESSION_V1` | `string` |  | Internal | ❌ |
| `XAVIER_TELEGRAM_BOT_TOKEN` | `string` |  | Internal | ✅ |
| `XAVIER_TELEGRAM_NOTIFICATION_CHAT_ID` | `string` |  | Internal | ❌ |
| `XAVIER_TELEMETRY_DB_PATH` | `string` |  | Internal | ❌ |
| `XAVIER_TEMPORAL_HALF_LIFE_DAYS` | `string` |  | Internal | ❌ |
| `XAVIER_TEST` | `string` |  | Internal | ❌ |
| `XAVIER_TEST_OLLAMA_PROBE_URL` | `string` |  | Internal | ❌ |
| `XAVIER_TEST_OLLAMA_REACHABLE_URL` | `string` |  | Internal | ❌ |
| `XAVIER_TEST_REAL_HOME` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_CONFIDENCE_THRESHOLD` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_ENABLED` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_ITERATIONS` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_LEARNING_RATE` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_MIN_NEW_HISTORY` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_REFINEMENT_BATCH_SIZE` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_REFINEMENT_THRESHOLD` | `string` |  | Internal | ❌ |
| `XAVIER_TGD_SCHEDULE` | `string` |  | Internal | ❌ |
| `XAVIER_TLS_CERT` | `string` |  | Internal | ❌ |
| `XAVIER_TLS_KEY` | `string` |  | Internal | ✅ |
| `XAVIER_TOKEN_VAULT_ENTRY` | `string` |  | Internal | ✅ |
| `XAVIER_TRAINING_MAX_RECORDS` | `string` |  | Internal | ❌ |
| `XAVIER_TUI_DUMP` | `string` |  | Internal | ❌ |
| `XAVIER_TUI_TAB` | `string` |  | Internal | ❌ |
| `XAVIER_VECTOR_WEIGHT` | `string` |  | Internal | ❌ |
| `XAVIER_VERSION` | `string` |  | Internal | ❌ |
| `XAVIER_WEEKLY_BUDGET` | `string` |  | Internal | ❌ |
| `XAVIER_WORKING_SNAPSHOT_INTERVAL_SECS` | `string` |  | Internal | ❌ |
| `XAVIER_ZAI_MODEL` | `string` |  | Internal | ❌ |
| `XAVIER_ZONE_BOOST` | `string` |  | Internal | ❌ |
| `XAVIER_ZONE_PENALTY` | `string` |  | Internal | ❌ |

## Vector Store Features

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_AUDIT_CHAIN_ENABLED` | `bool` | `true` | Enable audit chain validation | ❌ |
| `XAVIER_ENTITY_EXTRACTION_ENABLED` | `bool` | `true` | Enable entity extraction in vector store | ❌ |
| `XAVIER_QJL_THRESHOLD` | `usize` | `30000` | Vector count threshold for QJL quantization (must be > 0) | ❌ |
| `XAVIER_RRF_K` | `usize` | `60` | Reciprocal Rank Fusion k value (must be > 0) | ❌ |

## Workspace & Billing

| Variable | Type | Default | Description | Secret |
|----------|------|---------|-------------|--------|
| `XAVIER_DEFAULT_PLAN` | `string` | `"community"` | Billing plan: `community`, `pro`, `cloud`, `enterprise` | ❌ |
| `XAVIER_DEFAULT_WORKSPACE_ID` | `string` | `"default"` | Default workspace ID | ❌ |
| `XAVIER_EMBEDDING_PROVIDER_MODE` | `string` | `"bring_your_own"` | Embedding provider: `local`, `cloud`, `bring_your_own`, `disabled` | ❌ |
| `XAVIER_MANAGED_GOOGLE_EMBEDDINGS` | `bool` | `false` | Enable managed Google embeddings | ❌ |
| `XAVIER_REQUEST_LIMIT` | `usize?` | `null` | Optional request count cap | ❌ |
| `XAVIER_REQUEST_UNIT_LIMIT` | `u64?` | `null` | Optional request unit cap | ❌ |
| `XAVIER_STORAGE_LIMIT_BYTES` | `u64?` | `null` | Optional storage cap | ❌ |
| `XAVIER_SYNC_POLICY` | `string` | `"local_only"` | Memory sync policy | ❌ |
| `XAVIER_WORKSPACE_ID` | `string` | `"default"` | Legacy fallback workspace ID | ❌ |

<!-- END AUTOGEN -->
