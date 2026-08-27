//! Ollama native embedding API integration
//!
//! Provides native Ollama embedding generation via /api/embed endpoint
//! supporting local-first 768-dimensional models like nomic-embed-text.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::embedding::{Embedder, EmbeddingError};

/// Embedder implementation using Ollama native /api/embed API.
pub struct OllamaEmbedder {
    client: Client,
    endpoint: String,
    model: String,
    dimension: usize,
}

impl fmt::Debug for OllamaEmbedder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OllamaEmbedder")
            .field("model", &self.model)
            .field("endpoint", &self.endpoint)
            .field("dimension", &self.dimension)
            .finish()
    }
}

impl OllamaEmbedder {
    /// Create a new OllamaEmbedder instance.
    pub fn new(
        model: String,
        endpoint: String,
        dimension: usize,
        timeout: Duration,
    ) -> Result<Self, EmbeddingError> {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| EmbeddingError::Network(error.to_string()))?;

        Ok(Self {
            client,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model,
            dimension,
        })
    }

    /// Construct OllamaEmbedder from environment variables with defaults.
    pub fn from_env() -> Result<Self, EmbeddingError> {
        let endpoint = std::env::var("XAVIER_OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434/api/embed".to_string());
        let model =
            std::env::var("XAVIER_OLLAMA_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());
        let dimension = std::env::var("XAVIER_OLLAMA_DIMS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(768);

        let timeout_secs = crate::settings::XavierSettings::current()
            .embedding
            .timeout_secs;

        Self::new(
            model,
            endpoint,
            dimension,
            Duration::from_secs(timeout_secs),
        )
    }

    /// Get active model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[derive(Debug, Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    embeddings: Vec<Vec<f32>>,
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn encode(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/json")
            .json(&OllamaEmbedRequest {
                model: &self.model,
                input: text,
            })
            .send()
            .await
            .map_err(|error| EmbeddingError::Network(error.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body_text = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read response body>".to_string());
            return Err(EmbeddingError::Network(format!(
                "Ollama API returned HTTP status {}: {}",
                status, body_text
            )));
        }

        let body: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|error| EmbeddingError::Parse(error.to_string()))?;

        let vector = body.embeddings.into_iter().next().ok_or_else(|| {
            EmbeddingError::Parse("Ollama API returned empty embeddings array".to_string())
        })?;

        Ok(vector)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::FallbackEmbedder;
    use crate::memory::sqlite_vec_store::vector;
    use rusqlite::{params, Connection};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[test]
    fn test_ollama_provider_configuration_from_env() {
        let _guard = crate::settings::tests::ENV_LOCK.lock().unwrap();

        std::env::set_var("XAVIER_OLLAMA_URL", "http://127.0.0.1:11434/api/embed");
        std::env::set_var("XAVIER_OLLAMA_MODEL", "nomic-embed-text");
        std::env::set_var("XAVIER_OLLAMA_DIMS", "768");

        let embedder = OllamaEmbedder::from_env().unwrap();
        assert_eq!(embedder.endpoint(), "http://127.0.0.1:11434/api/embed");
        assert_eq!(embedder.model(), "nomic-embed-text");
        assert_eq!(embedder.dimension(), 768);

        std::env::remove_var("XAVIER_OLLAMA_URL");
        std::env::remove_var("XAVIER_OLLAMA_MODEL");
        std::env::remove_var("XAVIER_OLLAMA_DIMS");
    }

    #[tokio::test]
    async fn test_local_embedding_generation_mock_ollama() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/api/embed", server.url());

        let fake_vector: Vec<f32> = (0..768).map(|i| i as f32 * 0.001).collect();
        let body_json = serde_json::json!({
            "model": "nomic-embed-text",
            "embeddings": [fake_vector]
        });

        let mock = server
            .mock("POST", "/api/embed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body_json.to_string())
            .create_async()
            .await;

        let embedder = OllamaEmbedder::new(
            "nomic-embed-text".to_string(),
            mock_url,
            768,
            Duration::from_secs(5),
        )
        .unwrap();

        let vector = embedder.encode("test prompt").await.unwrap();
        assert_eq!(vector.len(), 768);
        assert_eq!(vector[0], 0.0);
        assert_eq!(embedder.dimension(), 768);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_fallback_to_remote_when_ollama_unavailable() {
        // Ollama server returns 500 error
        let mut ollama_server = mockito::Server::new_async().await;
        let ollama_url = format!("{}/api/embed", ollama_server.url());
        let _ollama_mock = ollama_server
            .mock("POST", "/api/embed")
            .with_status(500)
            .create_async()
            .await;

        // Remote OpenAI-compatible server returns valid 768-dim vector
        let mut remote_server = mockito::Server::new_async().await;
        let remote_url = format!("{}/v1/embeddings", remote_server.url());
        let fake_vector: Vec<f32> = vec![0.5; 768];
        let remote_body = serde_json::json!({
            "data": [{ "embedding": fake_vector }]
        });
        let _remote_mock = remote_server
            .mock("POST", "/v1/embeddings")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(remote_body.to_string())
            .create_async()
            .await;

        let ollama_embedder = Arc::new(
            OllamaEmbedder::new(
                "nomic-embed-text".to_string(),
                ollama_url,
                768,
                Duration::from_secs(2),
            )
            .unwrap(),
        );

        let remote_embedder = Arc::new(
            crate::embedding::openai::OpenAICompatibleEmbedder::new(
                Some("test-key".to_string()),
                "text-embedding-3-small".to_string(),
                remote_url,
                768,
                Duration::from_secs(2),
            )
            .unwrap(),
        );

        let fallback = FallbackEmbedder {
            embedders: vec![ollama_embedder, remote_embedder],
        };

        let result = fallback.encode("hello fallback").await.unwrap();
        assert_eq!(result.len(), 768);
        assert_eq!(result[0], 0.5);
    }

    #[tokio::test]
    async fn test_embedding_dimensions_match_768d() {
        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/api/embed", server.url());

        let fake_vector: Vec<f32> = vec![0.42; 768];
        let body_json = serde_json::json!({
            "model": "nomic-embed-text",
            "embeddings": [fake_vector]
        });

        let _mock = server
            .mock("POST", "/api/embed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body_json.to_string())
            .create_async()
            .await;

        let embedder = OllamaEmbedder::new(
            "nomic-embed-text".to_string(),
            mock_url,
            768,
            Duration::from_secs(5),
        )
        .unwrap();

        assert_eq!(embedder.dimension(), 768);
        assert_eq!(
            crate::embedding::embedding_dimension_for_model("nomic-embed-text"),
            768
        );

        let vector = embedder.encode("dimension check").await.unwrap();
        assert_eq!(vector.len(), embedder.dimension());
        assert_eq!(vector.len(), 768);
    }

    #[tokio::test]
    async fn test_connection_timeout_handling() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint = format!("http://{}/api/embed", addr);

        tokio::spawn(async move {
            if let Ok((_stream, _)) = listener.accept().await {
                // Sleep longer than the client timeout to trigger network timeout
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

        let embedder = OllamaEmbedder::new(
            "nomic-embed-text".to_string(),
            endpoint,
            768,
            Duration::from_millis(100),
        )
        .unwrap();

        let result = embedder.encode("timeout test").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EmbeddingError::Network(msg) => {
                assert!(
                    msg.contains("timed out") || msg.contains("timeout") || msg.contains("error"),
                    "unexpected error message: {}",
                    msg
                );
            }
            err => panic!("expected Network error, got {:?}", err),
        }
    }

    #[tokio::test]
    async fn test_backfill_idempotent_and_resumable() {
        let _ = vector::register_sqlite_vec_extension();
        let conn = Connection::open_in_memory().unwrap();

        // Create test schema
        conn.execute_batch(
            "CREATE TABLE memory_records (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                path TEXT NOT NULL,
                content TEXT NOT NULL,
                metadata TEXT NOT NULL DEFAULT '{}',
                embedding BLOB,
                created_at DATETIME NOT NULL,
                updated_at DATETIME NOT NULL,
                revision INTEGER NOT NULL DEFAULT 1,
                embedding_status TEXT DEFAULT 'pending',
                embedding_attempts INTEGER DEFAULT 0
            );
            CREATE TABLE memory_embeddings_768 (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL,
                embedding BLOB NOT NULL
            );
            CREATE TABLE backfill_checkpoint (
                key TEXT PRIMARY KEY,
                value TEXT,
                updated_at DATETIME
            );",
        )
        .unwrap();

        // Insert 10 records needing embeddings
        for i in 1..=10 {
            conn.execute(
                "INSERT INTO memory_records (id, workspace_id, path, content, created_at, updated_at) VALUES (?1, 'ws_1', ?2, ?3, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                params![format!("rec_{}", i), format!("path_{}", i), format!("Content of record {}", i)],
            ).unwrap();
        }

        // Mock Ollama server
        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/api/embed", server.url());
        let fake_vec: Vec<f32> = vec![0.1; 768];
        let body_json = serde_json::json!({
            "model": "nomic-embed-text",
            "embeddings": [fake_vec]
        });

        let _mock = server
            .mock("POST", "/api/embed")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body_json.to_string())
            .expect_at_least(1)
            .create_async()
            .await;

        let embedder = OllamaEmbedder::new(
            "nomic-embed-text".to_string(),
            mock_url,
            768,
            Duration::from_secs(5),
        )
        .unwrap();

        // Simulating backfill run 1 (Process first 5 records)
        let records: Vec<(i64, String, String, String)> = {
            let mut stmt = conn.prepare("SELECT rowid, id, workspace_id, content FROM memory_records WHERE (embedding IS NULL OR length(embedding) < 100) ORDER BY rowid ASC LIMIT 5").unwrap();
            stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
        };

        assert_eq!(records.len(), 5);
        let last_rowid = records.last().unwrap().0;

        for (_rowid, id, ws, content) in &records {
            let vec = embedder.encode(content).await.unwrap();
            let blob = vector::serialize_embedding(&vec);
            let json_vec = serde_json::to_string(&vec).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO memory_embeddings_768 (id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))",
                params![id, ws, json_vec],
            ).unwrap();
            conn.execute(
                "UPDATE memory_records SET embedding = ?1, embedding_status = 'completed' WHERE id = ?2",
                params![blob, id],
            ).unwrap();
        }

        // Save checkpoint
        conn.execute(
            "INSERT OR REPLACE INTO backfill_checkpoint (key, value, updated_at) VALUES ('last_processed_rowid', ?1, CURRENT_TIMESTAMP)",
            params![last_rowid.to_string()],
        ).unwrap();

        // Verify 5 embeddings in 768 table
        let count_768: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_embeddings_768", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count_768, 5);

        // Resume: fetch remaining records starting after checkpoint rowid
        let checkpoint_val: String = conn
            .query_row(
                "SELECT value FROM backfill_checkpoint WHERE key = 'last_processed_rowid'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let checkpoint_rowid: i64 = checkpoint_val.parse().unwrap();
        assert_eq!(checkpoint_rowid, last_rowid);

        let remaining: Vec<(i64, String, String, String)> = {
            let mut stmt = conn.prepare("SELECT rowid, id, workspace_id, content FROM memory_records WHERE (embedding IS NULL OR length(embedding) < 100) AND rowid > ? ORDER BY rowid ASC").unwrap();
            stmt.query_map(params![checkpoint_rowid], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
        };

        assert_eq!(remaining.len(), 5);

        // Process remaining records
        for (_rowid, id, ws, content) in &remaining {
            let vec = embedder.encode(content).await.unwrap();
            let blob = vector::serialize_embedding(&vec);
            let json_vec = serde_json::to_string(&vec).unwrap();
            conn.execute(
                "INSERT OR REPLACE INTO memory_embeddings_768 (id, workspace_id, embedding) VALUES (?1, ?2, vec_f32(?3))",
                params![id, ws, json_vec],
            ).unwrap();
            conn.execute(
                "UPDATE memory_records SET embedding = ?1, embedding_status = 'completed' WHERE id = ?2",
                params![blob, id],
            ).unwrap();
        }

        // Verify all 10 records are embedded
        let count_total_768: i64 = conn
            .query_row("SELECT COUNT(*) FROM memory_embeddings_768", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count_total_768, 10);

        // Idempotency check: running query for un-embedded records returns 0
        let un_embedded: i64 = conn.query_row("SELECT COUNT(*) FROM memory_records WHERE (embedding IS NULL OR length(embedding) < 100)", [], |r| r.get(0)).unwrap();
        assert_eq!(un_embedded, 0);
    }
}
