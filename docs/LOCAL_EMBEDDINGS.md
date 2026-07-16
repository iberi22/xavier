# Local Embeddings in Xavier

Xavier supports multiple modes for generating text embeddings locally. This document explains the differences between **GLLM** (in-process GPU/CPU) and **Ollama** (external process).

## Comparison: GLLM vs Ollama

| Feature | GLLM (Native) | Ollama (HTTP) |
|---------|---------------|---------------|
| **Deployment** | In-process (static binary) | External service (daemon) |
| **Hardware** | GPU (WGPU/CUDA) or CPU | GPU (diverse backends) or CPU |
| **Network** | None (direct memory access) | HTTP (localhost:11434) |
| **Latency** | Ultra-low | Low (HTTP overhead) |
| **Ease of Use** | Single binary, no setup | Requires installing Ollama |
| **Models** | Built-in support for Qwen3, MiniLM | Any model in Ollama library |

## GLLM (General Local LLM) Mode

GLLM is the preferred way to achieve 100% local operation without external dependencies. It uses the `candle` inference framework to run models directly within the Xavier process.

### Activation

To activate GLLM, set the following environment variable:

```bash
export XAVIER_EMBEDDING_PROVIDER_MODE=local-gllm
```

Or via `XavierSettings` (JSON/Tauri):
```json
{
  "workspace": {
    "embedding_provider_mode": "local-gllm"
  }
}
```

### Configuration

- **`XAVIER_GLLM_MODEL`**: The model identifier.
  - Recommended: `Qwen/Qwen3-Embedding-0.6B` (default).
  - Fast CPU: `all-MiniLM-L6-v2`.
- **`XAVIER_GLLM_MODEL_PATH`**: Path to a local model file (GGUF/Safetensors). If set, Xavier will validate the file exists before starting.
- **`XAVIER_GLLM_DIMENSION`**: The vector dimension. Automatically inferred for known models (1024 for Qwen3, 384 for MiniLM).

### Troubleshooting

- **"without the local-gllm feature"**: You are using a build of Xavier that doesn't have GLLM compiled in.
- **"requires model at X"**: The path specified in `XAVIER_GLLM_MODEL_PATH` is invalid.

## Ollama Mode

If you already have Ollama running, Xavier can connect to it using the standard OpenAI-compatible embeddings API.

### Activation

```bash
export XAVIER_EMBEDDING_PROVIDER_MODE=local
export XAVIER_EMBEDDING_MODEL=nomic-embed-text
```

## Recommended Models

| Model | Dimension | Performance | Use Case |
|-------|-----------|-------------|----------|
| **Qwen3-Embedding-0.6B** | 1024 | Excellent | Best quality/speed ratio on GPU |
| **all-MiniLM-L6-v2** | 384 | Very Fast | Low-resource or CPU-only |
| **bge-base-en-v1.5** | 768 | Balanced | General purpose |

---

## Integration Test Suite

The integration test suite is located in `tests/embedding_local_integration.rs`. It covers:

1.  **Dimension and Encoding**: Verifies that the embedder correctly communicates with the local API and returns vectors of the expected dimension (e.g., 768 for `embeddinggemma`).
2.  **Similarity Sanity**: Ensures that the embeddings produced have meaningful cosine similarity scores.
3.  **Fallback Logic**: Tests that the system can gracefully handle failures in the primary local embedding provider.
4.  **Full Chain Integration**: Verifies the complete flow from text input, through the embedder, into `VecSqliteMemoryStore`, and finally retrieval via hybrid search.

## Running the Tests

### Against Mock Server (Default)

By default, the tests run against a built-in mock server implemented with `axum`. This ensures that the tests can run in CI environments without needing a real Ollama instance.

```bash
cargo test --test embedding_local_integration
```

### Against Real Ollama Instance

To verify integration with a real Ollama instance, ensure Ollama is running and has the `embeddinggemma` model pulled:

```bash
ollama pull embeddinggemma
```

Then run the tests with the `XAVIER_TEST_OLLAMA` environment variable set to `1`:

```bash
XAVIER_TEST_OLLAMA=1 cargo test --test embedding_local_integration
```

The test will attempt to connect to `http://localhost:11434/v1/embeddings`.

### Test Configuration Environment Variables

The tests use the following environment variables to configure the embedding system:

-   `XAVIER_EMBEDDING_PROVIDER_MODE`: Set to `local` for these tests.
-   `XAVIER_EMBEDDING_LOCAL_URL`: The endpoint for the local embedding API.
-   `XAVIER_EMBEDDING_MODEL`: The model name to use (defaults to `embeddinggemma`).
-   `XAVIER_TEST_OLLAMA`: Set to `1` to enable testing against a real local Ollama instance.
