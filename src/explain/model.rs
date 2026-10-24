//! Explanation model backed by the configured model provider client.

use anyhow::{anyhow, Result};
use futures_util::future::BoxFuture;

use super::render::ExplanationModel;
use crate::agents::provider::client::ModelProviderClient;
use crate::agents::provider::ModelProviderConfig;

/// Sends explanation prompts through a [`ModelProviderClient`].
pub struct ProviderExplanationModel {
    client: ModelProviderClient,
}

impl ProviderExplanationModel {
    pub fn new(client: ModelProviderClient) -> Self {
        Self { client }
    }
}

/// Removes the API key and lease token from a provider error message.
fn redact(message: String, config: &ModelProviderConfig) -> String {
    [config.api_key.as_deref(), config.lease_token.as_deref()]
        .into_iter()
        .flatten()
        .filter(|secret| !secret.is_empty())
        .fold(message, |text, secret| text.replace(secret, "<redacted>"))
}

impl ExplanationModel for ProviderExplanationModel {
    fn complete<'a>(&'a self, system: &'a str, user: &'a str) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            match self.client.generate_text(system, user).await {
                Ok(response) => Ok(response.text),
                Err(err) => {
                    let message = format!(
                        "{} explanation request failed: {err:#}",
                        self.client.config.provider_label
                    );
                    Err(anyhow!(redact(message, &self.client.config)))
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "sk-explain-test-secret";

    #[tokio::test]
    async fn unreachable_provider_returns_error_without_secret() {
        // Port 9 on loopback is closed, so the request fails without leaving the machine.
        let config = ModelProviderConfig::from_label("openai")
            .with_api_key(Some(SECRET.to_string()))
            .with_base_url(Some("http://127.0.0.1:9/v1".to_string()));
        let model = ProviderExplanationModel::new(ModelProviderClient::new(config));

        let err = model
            .complete("system", "user")
            .await
            .expect_err("unreachable endpoint must fail");
        let message = format!("{err:#}");

        assert!(
            message.starts_with("openai explanation request failed"),
            "{message}"
        );
        assert!(!message.contains(SECRET), "secret leaked: {message}");
    }

    #[test]
    fn redact_removes_api_key_and_lease_token() {
        let mut config = ModelProviderConfig::from_label("openai");
        config.api_key = Some("key-1".to_string());
        config.lease_token = Some("lease-2".to_string());

        let out = redact("key-1 and lease-2 in url".to_string(), &config);

        assert_eq!(out, "<redacted> and <redacted> in url");
    }
}
