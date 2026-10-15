xavier::isolate_test_process!();

mod router_harness;

use std::sync::Arc;

use axum::{extract::Path, extract::State, http::StatusCode};
use router_harness::{create_test_cli_state, RestoreEnv};
use xavier::agents::rate_limit::RateLimitManager;
use xavier::agents::ui_render::UiRenderAgent;
use xavier::app::proxy_use_case::ProxyUseCase;
use xavier::cli::handlers::panel::{panel_delete_thread, panel_process_chat_inner};
use xavier::cli::state::CliState;
use xavier::codebase::connection_manager::ConnectionManager;
use xavier::codebase::conversations_db::ConversationsDb;
use xavier::server::panel::PanelChatRequest;

async fn create_panel_test_state(temp: &tempfile::TempDir) -> CliState {
    let mut state = create_test_cli_state(temp).await;
    let project_id = format!("test_panel_metrics_{}", uuid::Uuid::new_v4().simple());
    ConnectionManager::global()
        .connect_with_path(&project_id, temp.path().join("metrics.db"))
        .unwrap();
    let rate_manager = Arc::new(RateLimitManager::new_with_project(&project_id));
    // Match production startup: connecting alone does not initialize quota tables.
    rate_manager.init_schema_async().await.unwrap();
    state.rate_manager = rate_manager.clone();
    state.proxy_use_case = Arc::new(ProxyUseCase::new(rate_manager, state.prompt_cache.clone()));
    state
}

#[tokio::test]
async fn production_chat_persists_ui_render_agent_output() {
    let temp = tempfile::tempdir().unwrap();
    let mut server = mockito::Server::new_async().await;
    let _env = RestoreEnv::set_str(&[("XAVIER_LOCAL_LLM_URL", &format!("{}/v1", server.url()))]);
    let models = server
        .mock("GET", "/v1/models")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"object":"list","data":[]}"#)
        .create_async()
        .await;
    let answer = "Actual answer: <memory> & \"context\"";
    let chat = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": answer}}]
            })
            .to_string(),
        )
        .create_async()
        .await;
    let mut state = create_panel_test_state(&temp).await;
    state.panel_store = Arc::new(
        ConversationsDb::open_at("panel-test", &temp.path().join("panel.db"))
            .await
            .unwrap(),
    );
    for provider in [
        "opencode-go",
        "deepseek",
        "groq",
        "openrouter",
        "google",
        "openai",
        "anthropic",
        "managed-local",
    ] {
        state
            .proxy_use_case
            .rate_manager
            .report_429(provider, 30)
            .await
            .unwrap();
    }
    let query = "Explain the memory";
    let response = panel_process_chat_inner(
        &state,
        PanelChatRequest {
            thread_id: None,
            message: query.to_string(),
        },
    )
    .await
    .unwrap();
    models.assert_async().await;
    chat.assert_async().await;

    // Read back from SQLite, independently of the handler's returned messages.
    let messages = state
        .panel_store
        .get_thread_messages(&response.thread.id)
        .await
        .unwrap();
    assert_eq!(messages.len(), 2);
    let assistant = messages
        .iter()
        .find(|message| message.role == "assistant")
        .unwrap();
    assert_eq!(assistant.content, answer);
    let metadata: serde_json::Value =
        serde_json::from_str(assistant.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["source"], "llm");
    let rendered = UiRenderAgent::new().render_answer(answer);
    assert_eq!(
        assistant.openui_lang.as_deref(),
        Some(rendered.openui_lang.as_str())
    );
    assert_eq!(
        metadata["components"],
        serde_json::json!(rendered.components)
    );
    assert_eq!(metadata["rules"], serde_json::json!(rendered.rules_applied));
    assert!(!rendered.openui_lang.contains("<InfoCard title=\"Status\""));
    assert!(rendered
        .openui_lang
        .contains("&lt;memory&gt; &amp; &quot;context&quot;"));

    let deleted = panel_delete_thread(State(state.clone()), Path(response.thread.id.clone())).await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(state
        .panel_store
        .get_thread_messages(&response.thread.id)
        .await
        .unwrap()
        .is_empty());
    let missing = panel_delete_thread(State(state), Path(response.thread.id)).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

struct UnavailableProxy;

#[async_trait::async_trait]
impl xavier::ports::outbound::ThreatDetectionPort for UnavailableProxy {
    async fn scan_and_log(&self, _text: &str, _component: &str) -> anyhow::Result<bool> {
        anyhow::bail!("test proxy unavailable")
    }
}

#[tokio::test]
async fn production_chat_fallback_also_renders_actual_answer() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = create_panel_test_state(&temp).await;
    state.panel_store = Arc::new(
        ConversationsDb::open_at("fallback-test", &temp.path().join("panel.db"))
            .await
            .unwrap(),
    );
    state.proxy_use_case = Arc::new(
        xavier::app::proxy_use_case::ProxyUseCase::new(
            state.rate_manager.clone(),
            state.prompt_cache.clone(),
        )
        .with_threat_detector(Arc::new(UnavailableProxy)),
    );
    let response = panel_process_chat_inner(
        &state,
        PanelChatRequest {
            thread_id: None,
            message: "No matching memory".to_string(),
        },
    )
    .await
    .unwrap();
    let messages = state
        .panel_store
        .get_thread_messages(&response.thread.id)
        .await
        .unwrap();
    let assistant = messages
        .iter()
        .find(|message| message.role == "assistant")
        .unwrap();
    assert!(assistant.content.contains("test proxy unavailable"));
    let openui = assistant.openui_lang.as_deref().unwrap();
    let escaped_content = assistant
        .content
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    assert!(openui.contains(&escaped_content));
    assert!(!openui.contains("System Signals"));
    assert!(!openui.contains("TextCallout"));
    assert!(!openui.contains("<InfoCard title=\"Status\""));
    let metadata: serde_json::Value =
        serde_json::from_str(assistant.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(metadata["source"], "memory-fallback");
    assert!(metadata["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| rule == "always_show_summary"));
}
