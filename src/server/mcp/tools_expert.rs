//! MCP tool `ask_expert`: routes a prompt to the best active mini-expert.

use serde_json::{json, Value};

use super::types::{MCPTool, MCPToolResult};
use crate::agents::expert_router::{AskOutcome, ExpertRouter};
use crate::agents::provider::config::ModelProviderConfig;

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
                "domain": { "type": "string", "description": "Optional expert domain or name to target directly" },
                "provider": {
                    "type": "string",
                    "description": "per-call override of the general fallback model; API keys are taken from the server environment only.",
                    "enum": [
                        "local",
                        "managedlocal",
                        "managed-local",
                        "managed_local",
                        "cloud",
                        "disabled",
                        "anthropic",
                        "openai",
                        "deepseek",
                        "minimax",
                        "gemini",
                        "groq",
                        "z.ai",
                        "zai",
                        "opencode",
                        "openrouter"
                    ]
                },
                "model": {
                    "type": "string",
                    "description": "per-call override of the general fallback model; API keys are taken from the server environment only."
                }
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
    let prompt = match arguments.get("prompt") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim(),
        _ => anyhow::bail!("Invalid parameter 'prompt': must be a non-empty string"),
    };
    let domain = arguments
        .get("domain")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let domain_specified = domain.is_some();

    let provider = arguments
        .get("provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|p| !p.is_empty());

    let model = arguments
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|m| !m.is_empty());

    if let Some(p) = provider {
        if !ModelProviderConfig::is_known_label(p) {
            let err_val = json!({
                "error": format!("unknown provider '{p}'; expected one of local, managed-local, cloud, disabled, anthropic, openai, deepseek, minimax, gemini, groq, z.ai, opencode, openrouter")
            });
            return Ok(serde_json::to_value(MCPToolResult::structured(
                err_val, true,
            ))?);
        }
    }

    match router.ask(prompt, domain).await {
        Ok(outcome) if outcome.expert.is_none() => {
            let (mut payload, is_error) = match router.ask_general(prompt, provider, model).await {
                Ok((answer, provider_label, model_name)) => (
                    json!({
                        "answer": answer,
                        "expert": "general",
                        "version": null,
                        "score": null,
                        "matched_by": null,
                        "fallback": true,
                        "provider": provider_label,
                        "model": model_name,
                    }),
                    false,
                ),
                Err(e) => {
                    tracing::warn!("ask_expert: general-model fallback failed: {e}");
                    (
                        json!({
                            "answer": null,
                            "expert": "no expert",
                            "version": null,
                            "error": e.to_string(),
                        }),
                        true,
                    )
                }
            };
            if domain_specified {
                payload["domain_matched"] = json!(false);
            }
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, is_error,
            ))?)
        }
        Ok(outcome) => {
            let mut payload = outcome_payload(&outcome);
            if domain_specified {
                payload["domain_matched"] = json!(true);
            }
            let is_error = payload.get("answer").is_some_and(|a| a.is_null())
                && payload.get("error").is_some();
            Ok(serde_json::to_value(MCPToolResult::structured(
                payload, is_error,
            ))?)
        }
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
        assert!(crate::server::mcp::get_xavier_tools(false)
            .iter()
            .any(|t| t.name == "ask_expert"));
    }

    #[tokio::test]
    async fn invalid_prompt_codes() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let cfg = ExpertRouterConfig::default();
        let router = ExpertRouter::new(store, None, cfg);

        // Missing prompt
        let err_missing = run_ask_expert(&router, json!({})).await.unwrap_err();
        // Empty / whitespace prompt
        let err_empty = run_ask_expert(&router, json!({"prompt": "   "}))
            .await
            .unwrap_err();
        // Wrong-typed prompt (number, bool, array, null)
        let err_wrong_num = run_ask_expert(&router, json!({"prompt": 123}))
            .await
            .unwrap_err();
        let err_null = run_ask_expert(&router, json!({"prompt": null}))
            .await
            .unwrap_err();

        let msg_missing = err_missing.to_string();
        let msg_empty = err_empty.to_string();
        let msg_wrong = err_wrong_num.to_string();
        let msg_null = err_null.to_string();

        assert_eq!(msg_missing, msg_empty);
        assert_eq!(msg_empty, msg_wrong);
        assert_eq!(msg_wrong, msg_null);
        assert!(msg_missing.contains("'prompt'"));
        assert!(msg_missing.contains("Invalid") || msg_missing.contains("must be"));
    }

    #[tokio::test]
    async fn unknown_domain_flagged() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "general answer"
                        }
                    }]
                })
                .to_string(),
            )
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let mut general = ModelProviderConfig::local_from_env();
        general.base_url = Some(format!("{}/v1", server.url()));
        general.model = "test-model".to_string();
        let cfg = ExpertRouterConfig {
            // Mini-experts and routing hit the mock too, never a real Ollama.
            ollama_base_url: server.url(),
            general,
            ..Default::default()
        };
        let router = ExpertRouter::new(store.clone(), None, cfg);

        // Unknown domain -> domain_matched: false
        let res = run_ask_expert(&router, json!({"prompt": "hi", "domain": "unknown_domain"}))
            .await
            .unwrap();
        assert_eq!(res["isError"], false);
        assert_eq!(res["structuredContent"]["domain_matched"], false);
        assert_eq!(res["structuredContent"]["fallback"], true);
        assert_eq!(res["structuredContent"]["expert"], "general");
        assert_eq!(res["structuredContent"]["answer"], "general answer");

        // Known domain -> domain_matched: true (uses mini-expert /api/chat mock)
        let _m_expert = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"message":{"content":"42"}}"#)
            .create_async()
            .await;
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
        let res_known = run_ask_expert(&router, json!({"prompt": "2+2", "domain": "arithmetic"}))
            .await
            .unwrap();
        assert_eq!(res_known["isError"], false);
        assert_eq!(res_known["structuredContent"]["domain_matched"], true);
        assert_eq!(res_known["structuredContent"]["expert"], "math");
    }

    #[tokio::test]
    async fn no_answer_is_error() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/v1/chat/completions")
            .with_status(500)
            .with_body("boom")
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let mut general = ModelProviderConfig::local_from_env();
        general.base_url = Some(format!("{}/v1", server.url()));
        general.model = "test-model".to_string();
        let cfg = ExpertRouterConfig {
            // Mini-experts and routing hit the mock too, never a real Ollama.
            ollama_base_url: server.url(),
            general,
            ..Default::default()
        };
        let router = ExpertRouter::new(store, None, cfg);

        // When answer is null and error is set, isError must be true
        let res = run_ask_expert(&router, json!({"prompt": "hi"}))
            .await
            .unwrap();
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["answer"].is_null());
        assert_eq!(res["structuredContent"]["expert"], "no expert");
        // The HTTP status from the provider must reach the caller, but not the
        // request URL (some providers put the API key in the query string).
        let err = res["structuredContent"]["error"]
            .as_str()
            .unwrap_or_default();
        assert!(
            err.contains("500"),
            "error should carry the HTTP status, got: {err}"
        );
        assert!(
            !err.contains(&server.host_with_port()),
            "error must not leak the provider URL, got: {err}"
        );

        // Also flagged with unknown domain
        let res_domain = run_ask_expert(&router, json!({"prompt": "hi", "domain": "unknown"}))
            .await
            .unwrap();
        assert_eq!(res_domain["isError"], true);
        assert!(res_domain["structuredContent"]["answer"].is_null());
        assert_eq!(res_domain["structuredContent"]["domain_matched"], false);
    }

    #[tokio::test]
    async fn test_ask_expert_no_expert_and_domain() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/v1/chat/completions")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "42"
                        }
                    }]
                })
                .to_string(),
            )
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let mut general = ModelProviderConfig::local_from_env();
        general.base_url = Some(format!("{}/v1", server.url()));
        general.model = "test-model".to_string();
        let cfg = ExpertRouterConfig {
            ollama_base_url: server.url(),
            general,
            ..Default::default()
        };
        let router = ExpertRouter::new(store.clone(), None, cfg);

        let res = run_ask_expert(&router, json!({"prompt": "hi"}))
            .await
            .unwrap();
        assert_eq!(res["structuredContent"]["expert"], "general");
        assert_eq!(res["structuredContent"]["answer"], "42");

        let _m_exp = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"message":{"content":"42"}}"#)
            .create_async()
            .await;
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
        assert_eq!(res["structuredContent"]["domain_matched"], true);
        assert!(res["structuredContent"]["score"].is_null());

        assert!(run_ask_expert(&router, json!({"prompt": " "}))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_ask_expert_no_expert_falls_back_to_general_model() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_body(mockito::Matcher::PartialJson(
                json!({"model": "test-model"}),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "general answer"
                        }
                    }]
                })
                .to_string(),
            )
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let mut general = ModelProviderConfig::local_from_env();
        general.base_url = Some(format!("{}/v1", server.url()));
        general.model = "test-model".to_string();
        let cfg = ExpertRouterConfig {
            general,
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
        assert_eq!(res["structuredContent"]["provider"], "local");
        assert_eq!(res["structuredContent"]["model"], "test-model");
        assert!(res["structuredContent"]["error"].is_null());
    }

    #[tokio::test]
    async fn unconfigured_general_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let mut general = ModelProviderConfig::local_from_env();
        general.model = "".to_string();
        let cfg = ExpertRouterConfig {
            general,
            ..Default::default()
        };
        let router = ExpertRouter::new(store, None, cfg);

        let res = run_ask_expert(&router, json!({"prompt": "hi"}))
            .await
            .unwrap();
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["answer"].is_null());
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("XAVIER_LOCAL_LLM_MODEL"));
    }

    #[tokio::test]
    async fn override_model_arg_used() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/chat/completions")
            .match_body(mockito::Matcher::PartialJson(
                json!({"model": "other-model"}),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": "from other model"
                        }
                    }]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let mut general = ModelProviderConfig::local_from_env();
        general.base_url = Some(format!("{}/v1", server.url()));
        general.model = "test-model".to_string();
        let cfg = ExpertRouterConfig {
            general,
            ..Default::default()
        };
        let router = ExpertRouter::new(store, None, cfg);

        let res = run_ask_expert(&router, json!({"prompt": "hi", "model": "other-model"}))
            .await
            .unwrap();
        mock.assert_async().await;
        assert_eq!(res["isError"], false);
        assert_eq!(res["structuredContent"]["answer"], "from other model");
        assert_eq!(res["structuredContent"]["model"], "other-model");
    }

    #[tokio::test]
    async fn unknown_provider_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join(ExpertStore::DB_FILE)).unwrap();
        let cfg = ExpertRouterConfig::default();
        let router = ExpertRouter::new(store, None, cfg);

        let res = run_ask_expert(
            &router,
            json!({"prompt": "hi", "provider": "totally-bogus-provider"}),
        )
        .await
        .unwrap();
        assert_eq!(res["isError"], true);
        assert!(res["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("unknown provider 'totally-bogus-provider'"));
    }
}
