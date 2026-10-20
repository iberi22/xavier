//! MCP tool `ask_expert`: routes a prompt to the best active mini-expert.

use serde::Deserialize;
use serde_json::{json, Value};

use super::types::{MCPTool, MCPToolResult};
use crate::agents::expert_router::{AskOutcome, ExpertRouter};

#[derive(Debug, Deserialize)]
struct AskExpertArgs {
    prompt: String,
    #[serde(default)]
    domain: Option<String>,
}

pub fn get_expert_tools() -> Vec<MCPTool> {
    vec![MCPTool {
        name: "ask_expert".to_string(),
        description: "Ask a local mini-expert. Routes the prompt to the active expert whose \
            domain best matches (or the one named by `domain`). When none matches, the general \
            local model answers (`expert: \"general\"`, `fallback: true`). Returns the answer plus \
            which expert/version answered."
            .to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "prompt": { "type": "string", "description": "Question for the expert" },
                "domain": { "type": "string", "description": "Optional expert domain or name to target directly" }
            },
            "required": ["prompt"]
        }),
    }]
}

pub fn is_expert_tool(name: &str) -> bool {
    name == "ask_expert"
}

/// Shapes the MCP payload for an outcome.
pub fn outcome_payload(outcome: &AskOutcome) -> Value {
    match (&outcome.expert, &outcome.answer) {
        (Some(expert), Some(answer)) => json!({
            "answer": answer,
            "expert": expert,
            "version": outcome.version,
            "score": outcome.score,
            "matched_by": outcome.matched_by,
        }),
        _ => json!({ "answer": null, "expert": "no expert", "version": null }),
    }
}

/// Runs `ask_expert` against a prepared router (injectable for tests).
pub async fn run_ask_expert(router: &ExpertRouter, arguments: Value) -> anyhow::Result<Value> {
    let args: AskExpertArgs = serde_json::from_value(arguments)?;
    if args.prompt.trim().is_empty() {
        anyhow::bail!("ask_expert: 'prompt' must not be empty");
    }
    match router.ask(&args.prompt, args.domain.as_deref()).await {
        Ok(outcome) if outcome.expert.is_none() => {
            // No expert chosen: the general local model answers. A failure is degraded, not a tool error.
            let payload = match router.ask_general(&args.prompt).await {
                Ok(answer) => json!({
                    "answer": answer,
                    "expert": "general",
                    "version": null,
                    "score": null,
                    "matched_by": null,
                    "fallback": true,
                }),
                Err(e) => {
                    tracing::warn!("ask_expert: general-model fallback failed: {e}");
                    json!({ "answer": null, "expert": "no expert", "version": null, "error": e.to_string() })
                }
            };
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, false,
            ))?)
        }
        Ok(outcome) => Ok(serde_json::to_value(MCPToolResult::structured(
            outcome_payload(&outcome),
            false,
        ))?),
        Err(e) => Ok(serde_json::to_value(MCPToolResult::structured(
            json!({ "error": e.to_string() }),
            true,
        ))?),
    }
}

pub async fn handle_expert_tool(name: &str, arguments: Value) -> anyhow::Result<Value> {
    match name {
        "ask_expert" => {
            let router = ExpertRouter::from_env().await?;
            run_ask_expert(&router, arguments).await
        }
        other => Err(anyhow::anyhow!("Unknown expert tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::expert_router::ExpertRouterConfig;
    use crate::agents::mini_experts::{ExpertStore, NewExpert};

    #[test]
    fn test_tool_registered() {
        let tools = get_expert_tools();
        assert_eq!(tools[0].name, "ask_expert");
        assert!(is_expert_tool("ask_expert"));
        assert!(crate::server::mcp::get_xavier_tools()
            .iter()
            .any(|t| t.name == "ask_expert"));
    }

    #[tokio::test]
    async fn test_ask_expert_no_expert_and_domain() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"message":{"content":"42"}}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let cfg = ExpertRouterConfig {
            ollama_base_url: server.url(),
            ..Default::default()
        };
        let router = ExpertRouter::new(store.clone(), None, cfg);

        let res = run_ask_expert(&router, json!({"prompt": "hi"}))
            .await
            .unwrap();
        assert_eq!(res["structuredContent"]["expert"], "general");
        assert_eq!(res["structuredContent"]["answer"], "42");

        store
            .add_version(
                NewExpert {
                    name: "math".into(),
                    domain: "arithmetic".into(),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        let res = run_ask_expert(&router, json!({"prompt": "6*7", "domain": "arithmetic"}))
            .await
            .unwrap();
        assert_eq!(res["structuredContent"]["answer"], "42");
        assert_eq!(res["structuredContent"]["expert"], "math");
        assert_eq!(res["structuredContent"]["version"], "v1");
        assert_eq!(res["structuredContent"]["matched_by"], "domain");
        assert!(res["structuredContent"]["score"].is_null());

        assert!(run_ask_expert(&router, json!({"prompt": " "}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_ask_expert_no_expert_falls_back_to_general_model() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/chat")
            .match_body(mockito::Matcher::PartialJson(
                json!({"model": "qwen3-coder", "keep_alive": "5m"}),
            ))
            .with_status(200)
            .with_body(r#"{"message":{"content":"general answer"}}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let cfg = ExpertRouterConfig {
            ollama_base_url: server.url(),
            general_model: "qwen3-coder".into(),
            ..Default::default()
        };
        let router = ExpertRouter::new(store, None, cfg);

        let res = run_ask_expert(&router, json!({"prompt": "what is 2+2"}))
            .await
            .unwrap();
        mock.assert_async().await;
        assert_eq!(res["isError"], false);
        assert_eq!(res["structuredContent"]["answer"], "general answer");
        assert_eq!(res["structuredContent"]["expert"], "general");
        assert_eq!(res["structuredContent"]["fallback"], true);
        assert!(res["structuredContent"]["error"].is_null());
    }

    #[tokio::test]
    async fn test_ask_expert_general_model_failure_is_degraded_not_error() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/api/chat")
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let cfg = ExpertRouterConfig {
            ollama_base_url: server.url(),
            ..Default::default()
        };
        let router = ExpertRouter::new(store, None, cfg);

        let res = run_ask_expert(&router, json!({"prompt": "hi"}))
            .await
            .unwrap();
        assert_eq!(res["isError"], false);
        assert!(res["structuredContent"]["answer"].is_null());
        assert_eq!(res["structuredContent"]["expert"], "no expert");
        assert!(res["structuredContent"]["error"]
            .as_str()
            .is_some_and(|e| e.contains("500")));
    }
}
