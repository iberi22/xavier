//! Chronicle changelog generation logic
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::agents::provider::ModelProviderClient;
use crate::chronicle::prompts::{CHRONICLE_SYSTEM_PROMPT, CHRONICLE_USER_PROMPT_TEMPLATE};

/// Data input for the Chronicle generator, typically from harvest and redact phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChronicleInput {
    pub date: String,
    pub active_projects: usize,
    pub commits: usize,
    pub files_modified: usize,
    pub sessions: usize,
    pub raw_data: String,
}

pub struct ChronicleGenerator {
    llm_client: ModelProviderClient,
}

impl Default for ChronicleGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl ChronicleGenerator {
    /// New.
    pub fn new() -> Self {
        // Try to prefer MiniMax if available via environment or config
        let model_override = std::env::var("XAVIER_CHRONICLE_MODEL").ok().or_else(|| {
            // Heuristic: if MINIMAX_API_KEY is present, we might want to use MiniMax
            if std::env::var("MINIMAX_API_KEY").is_ok() {
                Some("MiniMax-Text-01".to_string())
            } else {
                None
            }
        });

        Self {
            llm_client: ModelProviderClient::from_model_override(model_override),
        }
    }

    /// Generate.
    pub async fn generate(&self, input: ChronicleInput) -> Result<String> {
        info!(date = %input.date, "Generating daily chronicle post");

        let input_json = serde_json::to_string_pretty(&input)?;
        let user_prompt = CHRONICLE_USER_PROMPT_TEMPLATE.replace("{{input_data}}", &input_json);

        let response = self
            .llm_client
            .generate_text(CHRONICLE_SYSTEM_PROMPT, &user_prompt)
            .await?;

        let processed = self.post_process(&response.text);

        Ok(processed)
    }

    fn post_process(&self, text: &str) -> String {
        let processed = text.trim().to_string();

        // Basic verification of structure
        let required_headers = [
            "# Daily Chronicle",
            "## Resumen del día",
            "## Decisiones Técnicas",
            "## Bugs y Lecciones",
            "## Métricas",
            "## Archivos destacados",
        ];

        for header in required_headers {
            if !processed.contains(header) {
                warn!(header = %header, "Generated chronicle is missing a required header");
            }
        }

        processed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chronicle_input_serialization() {
        let input = ChronicleInput {
            date: "2023-10-27".to_string(),
            active_projects: 2,
            commits: 5,
            files_modified: 10,
            sessions: 3,
            raw_data: "Some raw data".to_string(),
        };

        let serialized = serde_json::to_string(&input).expect("test assertion");
        assert!(serialized.contains("2023-10-27"));
        assert!(serialized.contains("\"active_projects\":2"));
    }

    #[test]
    fn test_post_process_verification() {
        let generator = ChronicleGenerator::new();
        let valid_post = r#"
# Daily Chronicle — 2023-10-27
## Resumen del día
Everything went well.
## Decisiones Técnicas
### Use Rust
It was good.
## Bugs y Lecciones
### Typo
Fixed it.
## Métricas
- Proyectos activos: 1
- Commits: 1
- Archivos modificados: 1
- Sesiones: 1
## Archivos destacados
- `src/lib.rs` — Updated something.
"#;
        let processed = generator.post_process(valid_post);
        assert_eq!(processed, valid_post.trim());
    }

    #[tokio::test]
    async fn test_generate_requires_config() {
        // Hermetic: point the local LLM at a closed port so the call fails fast
        // regardless of the machine's live setup. (Regression: with a working
        // local LLM at 127.0.0.1:11435 this test observed Ok and failed, while
        // CI — no LLM at all — passed. The test must not depend on the host.)
        let saved_url = std::env::var("XAVIER_LOCAL_LLM_URL").ok();
        let saved_flavor = std::env::var("XAVIER_API_FLAVOR").ok();
        std::env::set_var("XAVIER_LOCAL_LLM_URL", "http://127.0.0.1:9/v1");
        std::env::set_var("XAVIER_API_FLAVOR", "openai-compatible");

        let generator = ChronicleGenerator::new();
        let input = ChronicleInput {
            date: "2023-10-27".to_string(),
            active_projects: 1,
            commits: 1,
            files_modified: 1,
            sessions: 1,
            raw_data: "test".to_string(),
        };

        let result = generator.generate(input).await;

        match saved_url {
            Some(value) => std::env::set_var("XAVIER_LOCAL_LLM_URL", value),
            None => std::env::remove_var("XAVIER_LOCAL_LLM_URL"),
        }
        match saved_flavor {
            Some(value) => std::env::set_var("XAVIER_API_FLAVOR", value),
            None => std::env::remove_var("XAVIER_API_FLAVOR"),
        }

        assert!(result.is_err(), "expected an error with no reachable LLM");
    }
}
