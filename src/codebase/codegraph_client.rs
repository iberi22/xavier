//! Direct zero-overhead Unix Domain Socket (UDS) agent client for CodeGraph queries.
//!
//! Provides `CodeGraphUdsClient` to query CodeGraph with sub-millisecond latency
//! over Unix domain sockets, bypassing TCP loopback overhead.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Representation of a code symbol returned by CodeGraph queries over UDS.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SymbolRecord {
    pub name: String,
    pub kind: String,
    pub lang: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
}

/// CodeGraph statistics summary returned over UDS.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct GraphStats {
    pub total_files: u64,
    pub total_symbols: u64,
    pub total_imports: u64,
    pub languages: HashMap<String, u64>,
}

/// Client for communicating with the CodeGraph agent via Unix Domain Sockets.
#[derive(Debug, Clone)]
pub struct CodeGraphUdsClient {
    socket_path: PathBuf,
}

impl Default for CodeGraphUdsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGraphUdsClient {
    /// Creates a new `CodeGraphUdsClient` auto-resolving the socket path.
    pub fn new() -> Self {
        Self {
            socket_path: Self::resolve_socket_path(),
        }
    }

    /// Creates a `CodeGraphUdsClient` targeting a specific socket path.
    pub fn with_path(path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: path.as_ref().to_path_buf(),
        }
    }

    /// Returns the resolved Unix Domain Socket path.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Resolves the socket path according to environment or common locations.
    /// Priority order:
    /// 1. `XAVIER_CODEGRAPH_UDS_PATH` env var if non-empty
    /// 2. `/tmp/codegraph.sock` if it exists
    /// 3. `~/.xavier/run/codegraph.sock` if it exists
    /// 4. Default fallback: `/tmp/codegraph.sock`
    pub fn resolve_socket_path() -> PathBuf {
        if let Ok(env_path) = std::env::var("XAVIER_CODEGRAPH_UDS_PATH") {
            let trimmed = env_path.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }

        let default_tmp = PathBuf::from("/tmp/codegraph.sock");
        if default_tmp.exists() {
            return default_tmp;
        }

        if let Some(home) = dirs::home_dir() {
            let user_sock = home.join(".xavier").join("run").join("codegraph.sock");
            if user_sock.exists() {
                return user_sock;
            }
        }

        default_tmp
    }

    /// Checks whether the socket exists on the filesystem and is accessible.
    pub fn is_available(&self) -> bool {
        self.socket_path.exists()
    }

    /// Queries CodeGraph for symbols matching `query` up to `limit`.
    pub async fn find_symbols(&self, query: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let encoded_query =
            url::form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
        let uri = format!("/v1/symbols?q={}&limit={}", encoded_query, limit);
        let response_bytes = self.send_get_request(&uri).await?;
        let records: Vec<SymbolRecord> = serde_json::from_slice(&response_bytes)
            .context("failed to deserialize SymbolRecord array from UDS response")?;
        Ok(records)
    }

    /// Retrieves CodeGraph statistics.
    pub async fn get_stats(&self) -> Result<GraphStats> {
        let response_bytes = self.send_get_request("/v1/stats").await?;
        let stats: GraphStats = serde_json::from_slice(&response_bytes)
            .context("failed to deserialize GraphStats from UDS response")?;
        Ok(stats)
    }

    /// Internal lightweight zero-overhead HTTP GET helper over Tokio UnixStream.
    async fn send_get_request(&self, uri: &str) -> Result<Vec<u8>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| {
                format!(
                    "failed to connect to CodeGraph UDS socket at {}",
                    self.socket_path.display()
                )
            })?;

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nAccept: application/json\r\n\r\n",
            uri
        );

        stream.write_all(request.as_bytes()).await?;
        stream.flush().await?;

        let mut raw_response = Vec::new();
        stream.read_to_end(&mut raw_response).await?;

        parse_http_body(&raw_response)
    }
}

/// Helper function to parse HTTP/1.1 response bytes and extract the body.
fn parse_http_body(raw_response: &[u8]) -> Result<Vec<u8>> {
    let response_str = std::str::from_utf8(raw_response)
        .context("invalid UTF-8 in HTTP response from CodeGraph socket")?;

    let header_end_pos = response_str
        .find("\r\n\r\n")
        .ok_or_else(|| anyhow!("malformed HTTP response: missing header delimiter"))?;

    let headers_part = &response_str[..header_end_pos];
    let body_part = &raw_response[header_end_pos + 4..];

    let mut lines = headers_part.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| anyhow!("empty HTTP response status line"))?;

    if !status_line.contains("200") {
        return Err(anyhow!("HTTP error response from UDS: {}", status_line));
    }

    // Handle chunked transfer encoding if present
    let is_chunked = lines.any(|line| {
        let lower = line.to_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });

    if is_chunked {
        decode_chunked_body(body_part)
    } else {
        Ok(body_part.to_vec())
    }
}

/// Simple HTTP chunked body decoder.
fn decode_chunked_body(mut body: &[u8]) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();

    loop {
        let crlf_pos = body
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| anyhow!("invalid chunked encoding: missing chunk size CRLF"))?;

        let size_str = std::str::from_utf8(&body[..crlf_pos])
            .context("invalid chunk size string")?
            .trim();

        let chunk_size = usize::from_str_radix(size_str.split(';').next().unwrap_or(""), 16)
            .context("failed to parse chunk size hex")?;

        if chunk_size == 0 {
            break;
        }

        let chunk_start = crlf_pos + 2;
        let chunk_end = chunk_start + chunk_size;

        if body.len() < chunk_end + 2 {
            return Err(anyhow!("truncated chunk in UDS stream"));
        }

        decoded.extend_from_slice(&body[chunk_start..chunk_end]);
        body = &body[chunk_end + 2..];
    }

    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::net::UnixListener;

    #[test]
    fn test_socket_path_resolution_default() {
        let client = CodeGraphUdsClient::new();
        assert!(!client.socket_path().as_os_str().is_empty());
    }

    #[test]
    fn test_socket_path_resolution_env_override() {
        let temp_dir = tempdir().expect("tempdir failed");
        let mock_path = temp_dir.path().join("env_test.sock");

        std::env::set_var("XAVIER_CODEGRAPH_UDS_PATH", mock_path.to_str().unwrap());
        let resolved = CodeGraphUdsClient::resolve_socket_path();
        std::env::remove_var("XAVIER_CODEGRAPH_UDS_PATH");

        assert_eq!(resolved, mock_path);
    }

    #[test]
    fn test_client_is_available() {
        let temp_dir = tempdir().expect("tempdir failed");
        let mock_path = temp_dir.path().join("nonexistent.sock");
        let client = CodeGraphUdsClient::with_path(&mock_path);

        assert!(!client.is_available());

        // Create empty file at path to test availability
        std::fs::write(&mock_path, b"").unwrap();
        assert!(client.is_available());
    }

    #[tokio::test]
    async fn test_find_symbols_over_mock_uds() {
        let temp_dir = tempdir().expect("tempdir failed");
        let sock_path = temp_dir.path().join("test_symbols.sock");

        let listener = UnixListener::bind(&sock_path).expect("bind listener");

        // Spawn mock UDS HTTP server task
        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req_text = String::from_utf8_lossy(&buf[..n]);

                assert!(req_text.contains("GET /v1/symbols?q=test_query&limit=5 HTTP/1.1"));

                let mock_symbols = vec![SymbolRecord {
                    name: "test_query_fn".to_string(),
                    kind: "function".to_string(),
                    lang: "rust".to_string(),
                    file_path: "src/lib.rs".to_string(),
                    start_line: 10,
                    end_line: 25,
                    signature: Some("fn test_query_fn()".to_string()),
                }];

                let json_body = serde_json::to_string(&mock_symbols).unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json_body.len(),
                    json_body
                );

                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        let client = CodeGraphUdsClient::with_path(&sock_path);
        let symbols = client.find_symbols("test_query", 5).await.unwrap();

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "test_query_fn");
        assert_eq!(symbols[0].kind, "function");

        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_get_stats_over_mock_uds() {
        let temp_dir = tempdir().expect("tempdir failed");
        let sock_path = temp_dir.path().join("test_stats.sock");

        let listener = UnixListener::bind(&sock_path).expect("bind listener");

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                let req_text = String::from_utf8_lossy(&buf[..n]);

                assert!(req_text.contains("GET /v1/stats HTTP/1.1"));

                let mut languages = HashMap::new();
                languages.insert("rust".to_string(), 42);

                let mock_stats = GraphStats {
                    total_files: 10,
                    total_symbols: 100,
                    total_imports: 25,
                    languages,
                };

                let json_body = serde_json::to_string(&mock_stats).unwrap();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    json_body.len(),
                    json_body
                );

                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        let client = CodeGraphUdsClient::with_path(&sock_path);
        let stats = client.get_stats().await.unwrap();

        assert_eq!(stats.total_files, 10);
        assert_eq!(stats.total_symbols, 100);
        assert_eq!(stats.total_imports, 25);
        assert_eq!(stats.languages.get("rust"), Some(&42));

        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_chunked_transfer_encoding_decoding() {
        let temp_dir = tempdir().expect("tempdir failed");
        let sock_path = temp_dir.path().join("test_chunked.sock");

        let listener = UnixListener::bind(&sock_path).expect("bind listener");

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let chunk1 = "[\"hello";
                let chunk2 = "\",\"world\"]";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n{:x}\r\n{}\r\n0\r\n\r\n",
                    chunk1.len(),
                    chunk1,
                    chunk2.len(),
                    chunk2
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        let client = CodeGraphUdsClient::with_path(&sock_path);
        let bytes = client.send_get_request("/v1/test").await.unwrap();
        let text = String::from_utf8(bytes).unwrap();

        assert_eq!(text, "[\"hello\",\"world\"]");

        let _ = server_task.await;
    }

    #[tokio::test]
    async fn test_http_error_handling() {
        let temp_dir = tempdir().expect("tempdir failed");
        let sock_path = temp_dir.path().join("test_err.sock");

        let listener = UnixListener::bind(&sock_path).expect("bind listener");

        let server_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf).await;
                let response = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });

        let client = CodeGraphUdsClient::with_path(&sock_path);
        let res = client.get_stats().await;

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("HTTP error response") || err_msg.contains("500"),
            "unexpected error message: {}",
            err_msg
        );

        let _ = server_task.await;
    }
}
