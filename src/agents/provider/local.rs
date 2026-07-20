//! Local Model Provider (Ollama integration).
//!
//! # Contract
//! When `ProviderMode::Local` is active, the system routes requests to a local instance of Ollama:
//! - **Base URL**: Defaults to `http://localhost:11434/v1` (`DEFAULT_LOCAL_BASE_URL`).
//! - **API Format**: OpenAI-compatible format, interacting with the `/v1/chat/completions` endpoint.
//! - **Auth Headers**: By default, no `Authorization` header is sent since Ollama does not require authentication.
//!   However, if `OLLAMA_API_KEY` or `XAVIER_LOCAL_LLM_API_KEY` is set in the environment,
//!   that key will be included in a `Bearer` token header.
//! - **Default Model**: Defaults to `qwen3-coder` (`DEFAULT_LOCAL_MODEL`).

/// The default local base URL for Ollama's OpenAI-compatible endpoint.
pub const DEFAULT_LOCAL_BASE_URL: &str = "http://localhost:11434/v1";

/// The default local model name for code and general assistance.
pub const DEFAULT_LOCAL_MODEL: &str = "qwen3-coder";

#[cfg(test)]
mod tests {
    use crate::agents::provider::client::ModelProviderClient;
    use crate::agents::provider::config::ModelProviderConfig;
    use crate::agents::provider::types::{ApiFlavor, ProviderMode, ProviderTarget};
    use serde_json::json;
    use std::sync::Mutex;

    // Mutex to serialize environment variable manipulation in tests
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_local_provider_request_without_auth() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // Ensure env variables are not present
        std::env::remove_var("OLLAMA_API_KEY");
        std::env::remove_var("XAVIER_LOCAL_LLM_API_KEY");

        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/v1", server.url());

        // Create a ModelProviderConfig for Local mode
        let config = ModelProviderConfig {
            provider_mode: ProviderMode::Local,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "local".to_string(),
            model: "qwen3-coder".to_string(),
            api_key: Some("ollama".to_string()), // default key, should be filtered out
            base_url: Some(mock_url),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            lease_token: None,
            secret_injection_strategy: None,
        };

        let client = ModelProviderClient::new(config);

        // Prepare the mock response
        let response_body = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "qwen3-coder",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from mock local Ollama!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 9,
                "completion_tokens": 12,
                "total_tokens": 21
            }
        });

        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("Content-Type", "application/json")
            // Verify there is NO Authorization header
            .match_header("Authorization", mockito::Matcher::Missing)
            .match_body(mockito::Matcher::PartialJson(json!({
                "model": "qwen3-coder",
                "messages": [
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": "Hello"}
                ]
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&response_body).unwrap())
            .create_async()
            .await;

        let res = client
            .generate_text("You are a helpful assistant.", "Hello")
            .await;
        assert!(res.is_ok(), "Failed to generate response: {:?}", res.err());
        let res = res.unwrap();
        assert_eq!(res.text, "Hello from mock local Ollama!");

        mock.assert_async().await;
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn test_local_provider_request_with_ollama_api_key() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // Set OLLAMA_API_KEY
        std::env::set_var("OLLAMA_API_KEY", "ollama-secret-token");
        std::env::remove_var("XAVIER_LOCAL_LLM_API_KEY");

        let mut server = mockito::Server::new_async().await;
        let mock_url = format!("{}/v1", server.url());

        let config = ModelProviderConfig {
            provider_mode: ProviderMode::Local,
            api_flavor: ApiFlavor::OpenAICompatible,
            provider_label: "local".to_string(),
            model: "qwen3-coder".to_string(),
            api_key: None,
            base_url: Some(mock_url),
            target: ProviderTarget::GenericOpenAICompatible,
            lease_config: None,
            lease_token: None,
            secret_injection_strategy: None,
        };

        let client = ModelProviderClient::new(config);

        let response_body = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Authenticated response!"
                }
            }]
        });

        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_header("Content-Type", "application/json")
            // Verify the Authorization header contains Bearer ollama-secret-token
            .match_header("Authorization", "Bearer ollama-secret-token")
            .match_body(mockito::Matcher::PartialJson(json!({
                "model": "qwen3-coder"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::to_string(&response_body).unwrap())
            .create_async()
            .await;

        let res = client.generate_text("System", "User").await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().text, "Authenticated response!");

        mock.assert_async().await;
        std::env::remove_var("OLLAMA_API_KEY");
    }
}
