# Local Embeddings Integration Tests

This document describes how to run and maintain integration tests for local embeddings in Xavier, specifically targeting Ollama and other OpenAI-compatible local providers.

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

## Configuration

The tests use the following environment variables to configure the embedding system:

-   `XAVIER_EMBEDDING_PROVIDER_MODE`: Set to `local` for these tests.
-   `XAVIER_EMBEDDING_LOCAL_URL`: The endpoint for the local embedding API.
-   `XAVIER_EMBEDDING_MODEL`: The model name to use (defaults to `embeddinggemma`).
-   `XAVIER_TEST_OLLAMA`: Set to `1` to enable testing against a real local Ollama instance.
