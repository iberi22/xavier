//! CLI and HTTP handlers for mini-experts (registry v2, SQLite).

use crate::cli::commands::enums::MiniExpertCommand;
use anyhow::{Context, Result};
use axum::{
    extract::Path as AxumPath,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use xavier::agents::expert_router::ExpertRouter;
use xavier::agents::mini_experts::{
    ensure_active_in_ollama, CliOllamaRunner, EnsureOutcome, ExpertRecord, ExpertStore, NewExpert,
    OllamaRunner,
};

fn print_records(list: &[ExpertRecord]) {
    if list.is_empty() {
        println!("No mini-experts registered.");
        return;
    }
    println!("Registered Mini-Experts ({} versions):", list.len());
    for r in list {
        println!(
            " - {}@{} [{}] domain: {}, clearance: {}, ollama: {}, GGUF: {}",
            r.name,
            r.version,
            r.status.as_str(),
            r.domain,
            r.clearance,
            r.ollama_model,
            r.gguf_path
        );
    }
}

/// Handles mini-expert CLI subcommands.
pub async fn handle_mini_expert_command(cmd: MiniExpertCommand) -> Result<()> {
    let store = ExpertStore::open_default()?;
    run_command(&store, &CliOllamaRunner::from_env(), cmd).await
}

/// Command logic with injected store and Ollama runner.
pub async fn run_command(
    store: &ExpertStore,
    runner: &dyn OllamaRunner,
    cmd: MiniExpertCommand,
) -> Result<()> {
    match cmd {
        MiniExpertCommand::Add {
            name,
            segment,
            language,
            clearance,
            source_dataset,
            model_gguf_path,
            provider,
            endpoint,
            version,
            metrics_file,
            domain,
            candidate,
        } => {
            if !matches!(provider.trim(), "" | "local" | "ollama") {
                eprintln!(
                    "warning: provider '{provider}' is stored, but the expert router and \
                     `ask_expert` always call the local Ollama; only the legacy HTTP invoke \
                     route uses the provider/endpoint."
                );
            }
            let metrics_json = match metrics_file {
                Some(path) => std::fs::read_to_string(&path)
                    .with_context(|| format!("Failed to read metrics file {}", path.display()))?,
                None => String::new(),
            };
            let rec = store.add_version(
                NewExpert {
                    name: name.clone(),
                    version,
                    domain: domain.unwrap_or(segment),
                    gguf_path: model_gguf_path,
                    metrics_json,
                    clearance,
                    language,
                    source_dataset,
                    provider,
                    endpoint,
                    ..Default::default()
                },
                !candidate,
            )?;
            println!(
                "Successfully registered mini-expert '{}' version {} ({}).",
                name,
                rec.version,
                if candidate { "candidate" } else { "active" }
            );
            if !candidate {
                println!("Run `xavier mini-expert serve` to make sure Ollama has it.");
            }
        }
        MiniExpertCommand::List => print_records(&store.list()?),
        MiniExpertCommand::Activate { name, version } => {
            store.activate(&name, &version)?;
            println!("Activated {name}@{version}.");
        }
        MiniExpertCommand::Retire { name, version } => {
            if store.retire(&name, version.as_deref())? {
                println!(
                    "Retired {name}{}.",
                    version.map(|v| format!("@{v}")).unwrap_or_default()
                );
            } else {
                println!("Nothing to retire for '{name}'.");
            }
        }
        MiniExpertCommand::Serve { name, port } => {
            if port.is_some() {
                anyhow::bail!(
                    "`serve --port` is not supported: Ollama's address comes from \
                     XAVIER_LOCAL_LLM_URL"
                );
            }
            let reports = ensure_active_in_ollama(store, runner, name.as_deref()).await?;
            if reports.is_empty() {
                println!("No active mini-experts to serve.");
            }
            for r in reports {
                let status = match r.outcome {
                    EnsureOutcome::AlreadyPresent => "present".to_string(),
                    EnsureOutcome::Created => "created".to_string(),
                    EnsureOutcome::Failed(why) => format!("FAILED: {why}"),
                };
                println!(
                    "{}@{} -> ollama '{}': {}",
                    r.name, r.version, r.ollama_model, status
                );
            }
        }
    }
    Ok(())
}

fn error_response(status: StatusCode, msg: impl ToString) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
        .into_response()
}

/// GET /v1/agents/mini-experts — every registered version.
pub async fn list_experts_handler() -> Response {
    match ExpertStore::open_default().and_then(|s| s.list()) {
        Ok(list) => Json(list).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// GET /v1/agents/mini-experts/{name} — all versions of one expert.
pub async fn get_expert_handler(AxumPath(name): AxumPath<String>) -> Response {
    match ExpertStore::open_default().and_then(|s| s.versions(&name)) {
        Ok(v) if v.is_empty() => error_response(
            StatusCode::NOT_FOUND,
            format!("Mini-expert '{name}' not found"),
        ),
        Ok(v) => Json(v).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub prompt: String,
    /// Invoke this expert's active version directly (skips routing).
    #[serde(default)]
    pub name: Option<String>,
    /// Routing hint when `name` is absent.
    #[serde(default)]
    pub domain: Option<String>,
}

/// POST /v1/agents/mini-experts/invoke
pub async fn invoke_expert_handler(Json(req): Json<InvokeRequest>) -> Response {
    match ExpertRouter::from_env().await {
        Ok(router) => invoke_with(&router, req).await,
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Invoke logic against a prepared router.
pub async fn invoke_with(router: &ExpertRouter, req: InvokeRequest) -> Response {
    if req.prompt.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "prompt must not be empty");
    }
    if let Some(name) = req.name.as_deref() {
        let rec = match router.store().active(name) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return error_response(
                    StatusCode::NOT_FOUND,
                    format!("No active mini-expert '{name}'"),
                )
            }
            Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, e),
        };
        return match router.invoke_record(&rec, &req.prompt).await {
            Ok(answer) => Json(serde_json::json!({
                "status": "success", "expert": rec.name, "version": rec.version, "response": answer
            }))
            .into_response(),
            Err(e) => error_response(StatusCode::BAD_GATEWAY, e),
        };
    }
    match router.ask(&req.prompt, req.domain.as_deref()).await {
        Ok(o) if o.expert.is_some() => Json(serde_json::json!({
            "status": "success", "expert": o.expert, "version": o.version,
            "score": o.score, "matched_by": o.matched_by, "response": o.answer
        }))
        .into_response(),
        Ok(_) => Json(serde_json::json!({
            "status": "no_expert", "expert": null, "version": null, "response": null
        }))
        .into_response(),
        Err(e) => error_response(StatusCode::BAD_GATEWAY, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use xavier::agents::expert_router::ExpertRouterConfig;

    struct FakeRunner {
        created: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl OllamaRunner for FakeRunner {
        async fn list_models(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
        async fn create(&self, model: &str, _modelfile: &str) -> Result<()> {
            self.created.lock().unwrap().push(model.to_string());
            Ok(())
        }
    }

    fn add_cmd(
        name: &str,
        version: Option<&str>,
        gguf: &str,
        candidate: bool,
    ) -> MiniExpertCommand {
        MiniExpertCommand::Add {
            name: name.into(),
            segment: "general".into(),
            language: "en".into(),
            clearance: 1,
            source_dataset: "default".into(),
            model_gguf_path: gguf.into(),
            provider: "local".into(),
            endpoint: "http://localhost:11434/v1".into(),
            version: version.map(String::from),
            metrics_file: None,
            domain: Some("rust".into()),
            candidate,
        }
    }

    #[tokio::test]
    async fn test_cli_add_activate_retire_serve() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join("m.db")).unwrap();
        let gguf = dir.path().join("x.gguf");
        std::fs::write(&gguf, b"g").unwrap();
        let runner = FakeRunner {
            created: Mutex::new(vec![]),
        };

        // Legacy-style add (script compat): no version -> auto v1, active.
        run_command(
            &store,
            &runner,
            add_cmd("e", None, gguf.to_str().unwrap(), false),
        )
        .await
        .unwrap();
        let metrics = dir.path().join("metrics.json");
        std::fs::write(&metrics, r#"{"score":0.9}"#).unwrap();
        let mut cmd = add_cmd("e", Some("v2"), "", true);
        if let MiniExpertCommand::Add { metrics_file, .. } = &mut cmd {
            *metrics_file = Some(metrics);
        }
        run_command(&store, &runner, cmd).await.unwrap();
        assert_eq!(store.active("e").unwrap().unwrap().version, "v1");
        assert_eq!(
            store.get_version("e", "v2").unwrap().unwrap().metrics_json,
            r#"{"score":0.9}"#
        );

        run_command(
            &store,
            &runner,
            MiniExpertCommand::Activate {
                name: "e".into(),
                version: "v2".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(store.active("e").unwrap().unwrap().version, "v2");
        run_command(
            &store,
            &runner,
            MiniExpertCommand::Activate {
                name: "e".into(),
                version: "v1".into(),
            },
        )
        .await
        .unwrap();
        run_command(
            &store,
            &runner,
            MiniExpertCommand::Serve {
                name: None,
                port: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(runner.created.lock().unwrap().as_slice(), ["e:v1"]);
        run_command(
            &store,
            &runner,
            MiniExpertCommand::Retire {
                name: "e".into(),
                version: None,
            },
        )
        .await
        .unwrap();
        assert!(store.active("e").unwrap().is_none());
        run_command(&store, &runner, MiniExpertCommand::List)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_cli_add_persists_all_fields_and_serve_rejects_port() {
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join("m.db")).unwrap();
        let runner = FakeRunner {
            created: Mutex::new(vec![]),
        };
        let mut cmd = add_cmd("e", None, "", false);
        if let MiniExpertCommand::Add {
            language,
            source_dataset,
            provider,
            endpoint,
            ..
        } = &mut cmd
        {
            *language = "es".into();
            *source_dataset = "ds-9".into();
            *provider = "agy".into();
            *endpoint = "http://gpu-box:11434/v1".into();
        }
        run_command(&store, &runner, cmd).await.unwrap();
        let rec = store.active("e").unwrap().unwrap();
        assert_eq!(rec.language, "es");
        assert_eq!(rec.source_dataset, "ds-9");
        assert_eq!(rec.provider, "agy");
        assert_eq!(rec.endpoint, "http://gpu-box:11434/v1");

        let err = run_command(
            &store,
            &runner,
            MiniExpertCommand::Serve {
                name: None,
                port: Some(1234),
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("--port"));
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        use http_body_util::BodyExt;
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn test_invoke_handler_paths() {
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("POST", "/api/chat")
            .with_status(200)
            .with_body(r#"{"message":{"content":"hola"}}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let store = ExpertStore::open(dir.path().join("m.db")).unwrap();
        store
            .add_version(
                NewExpert {
                    name: "e".into(),
                    domain: "rust".into(),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        let cfg = ExpertRouterConfig {
            ollama_base_url: server.url(),
            ..Default::default()
        };
        let router = ExpertRouter::new(store, None, cfg);
        let req = |name: Option<&str>, domain: Option<&str>| InvokeRequest {
            prompt: "p".into(),
            name: name.map(String::from),
            domain: domain.map(String::from),
        };

        let resp = invoke_with(&router, req(Some("e"), None)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "success");
        assert_eq!(body["response"], "hola");
        assert_eq!(
            invoke_with(&router, req(Some("zz"), None)).await.status(),
            StatusCode::NOT_FOUND
        );
        let resp = invoke_with(&router, req(None, Some("rust"))).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "success");
        assert_eq!(body["expert"], "e");
        // No embedder + no domain -> "no_expert" (200, caller falls back).
        let resp = invoke_with(&router, req(None, None)).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "no_expert");
        assert!(body["response"].is_null());
        let empty = InvokeRequest {
            prompt: " ".into(),
            name: None,
            domain: None,
        };
        assert_eq!(
            invoke_with(&router, empty).await.status(),
            StatusCode::BAD_REQUEST
        );
    }
}
