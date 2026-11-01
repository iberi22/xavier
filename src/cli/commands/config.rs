//! Local settings inspection and runtime override commands.
use crate::settings::inspection::{ConfigError, Inspection};
use clap::{Args, Subcommand};
use serde_json::{json, Value};
use std::io::{IsTerminal, Read};
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
#[command(
    after_help = "Examples:\n  xavier config list --json\n  xavier config get server.port --json\n  xavier config set server.port 8200\n  xavier config set license.commercial_key --value-file /run/license\n  xavier config unset server.port\n  xavier config validate --json\n  xavier config path --json\n  xavier config explain models.embedding_model --json"
)]
pub struct ConfigArgs {
    /// Emit one versioned JSON document on stdout; diagnostics go to stderr
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommand {
    /// List effective dotted settings, with their sources
    List,
    /// Read an effective setting and its source
    Get { key: String },
    /// Write a typed runtime override; the config file stays read-only
    Set {
        key: String,
        /// Non-sensitive value (lists use JSON array syntax)
        value: Option<String>,
        /// Read a value from a file, required for sensitive settings
        #[arg(long)]
        value_file: Option<PathBuf>,
        /// Read a value from stdin without prompting
        #[arg(long)]
        value_stdin: bool,
    },
    /// Remove a runtime override, revealing the lower-priority value
    Unset { key: String },
    /// Check files, environment types and paths that depend on cwd
    Validate,
    /// Manage LLM configuration
    Llm {
        #[command(subcommand)]
        cmd: LlmConfigCommand,
    },
    /// Show the config and runtime-state file paths
    Path,
    /// Explain a setting's source, override variable and restart requirement
    Explain { key: String },
    /// Configure LLM routing and behavior
    Llm {
        #[command(subcommand)]
        cmd: ConfigLlmCommand,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigLlmCommand {
    /// Manage LLM routing configuration
    Route {
        #[command(subcommand)]
        cmd: ConfigLlmRouteCommand,
    },
}

#[derive(Args, Debug, Clone)]
pub struct ConfigLlmRouteCommandArgs {
    #[command(subcommand)]
    pub command: ConfigLlmRouteCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigLlmRouteCommand {
    /// List configured LLM routes
    List,
    /// Set a route for a specific task
    Set {
        /// The task name (e.g. `summarize`)
        task: String,
        /// The target provider and model (e.g. `local/llama3`)
        target: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum LlmConfigCommand {
    /// Set the LLM model
    #[command(name = "set-model")]
    SetModel { model: String },
    /// Show current LLM configuration
    Show,
}

fn error(code: i32, message: &str) -> ConfigError {
    ConfigError {
        code,
        message: message.into(),
    }
}
async fn execute(args: &ConfigArgs) -> Result<(Value, i32), ConfigError> {
    let mut inspection = Inspection::load();
    let mut body = json!({
        "schema_version": 1, "ok": true,
        "config_path": inspection.config_path,
        "runtime_state_path": inspection.runtime_state_path,
        "restart_required": false,
    });
    match &args.command {
        ConfigCommand::Llm {
            cmd: ConfigLlmCommand::Route { cmd },
        } => match cmd {
            ConfigLlmRouteCommand::List => {
                inspection.require_valid()?;
                body["operation"] = json!("list_routes");
                let routes = inspection
                    .effective()
                    .pointer("/models/routes")
                    .cloned()
                    .unwrap_or(json!({}));
                body["routes"] = routes;
            }
            ConfigLlmRouteCommand::Set { task, target } => {
                inspection.require_valid()?;
                let pointer = "/models/routes";
                let mut routes = inspection
                    .runtime()
                    .pointer(pointer)
                    .cloned()
                    .unwrap_or(json!({}));
                if let Some(map) = routes.as_object_mut() {
                    map.insert(task.clone(), json!(target));
                } else {
                    let mut map = serde_json::Map::new();
                    map.insert(task.clone(), json!(target));
                    routes = json!(map);
                }
                inspection.set_json("models.routes", routes)?;
                body["operation"] = json!("set_route");
                body["task"] = json!(task);
                body["target"] = json!(target);
                body["restart_required"] = json!(true);
            }
        },

        ConfigCommand::Path => {
            body["operation"] = json!("path");
        }

        ConfigCommand::Llm { cmd } => match cmd {
            LlmConfigCommand::SetModel { model } => {
                inspection.set("models.general_model", &model, false)?;
                body["operation"] = json!("set-model");
                body["model"] = json!(model);
                body["restart_required"] = json!(true);
            }
            LlmConfigCommand::Show => {
                let entry = inspection.entry("models.general_model")?;
                inspection.require_valid()?;
                body["operation"] = json!("show");
                body["entry"] = serde_json::to_value(entry).expect("entry serializes");
            }
        },
        ConfigCommand::Validate => {
            inspection.validate();
            body["operation"] = json!("validate");
            body["ok"] = json!(inspection.problems.is_empty());
            body["problems"] =
                serde_json::to_value(&inspection.problems).expect("problems serialize");
            return Ok((body, if inspection.problems.is_empty() { 0 } else { 1 }));
        }
        ConfigCommand::List => {
            inspection.require_valid()?;
            body["operation"] = json!("list");
            body["entries"] =
                serde_json::to_value(inspection.entries()).expect("entries serialize");
        }
        ConfigCommand::Get { key } | ConfigCommand::Explain { key } => {
            // Check key first so an unknown key is always a usage error.
            let entry = inspection.entry(key)?;
            inspection.require_valid()?;
            let explain = matches!(args.command, ConfigCommand::Explain { .. });
            body["operation"] = json!(if explain { "explain" } else { "get" });
            body["entry"] = serde_json::to_value(entry).expect("entry serializes");
            body["restart_required"] = json!(explain);
        }
        ConfigCommand::Set {
            key,
            value,
            value_file,
            value_stdin,
        } => {
            if usize::from(value.is_some())
                + usize::from(value_file.is_some())
                + usize::from(*value_stdin)
                != 1
            {
                return Err(error(
                    2,
                    "provide exactly one VALUE, --value-file PATH or --value-stdin",
                ));
            }
            let secure_input = value_file.is_some() || *value_stdin;
            let raw = if let Some(path) = value_file {
                std::fs::read_to_string(path).map_err(|_| error(1, "cannot read value file"))?
            } else if *value_stdin {
                if std::io::stdin().is_terminal() {
                    return Err(error(
                        2,
                        "pipe a value to --value-stdin or use --value-file PATH",
                    ));
                }
                let mut raw = String::new();
                std::io::stdin()
                    .read_to_string(&mut raw)
                    .map_err(|_| error(1, "cannot read value from stdin"))?;
                raw
            } else {
                value.clone().unwrap_or_default()
            };
            let raw = if secure_input {
                raw.trim_end_matches(['\r', '\n'])
            } else {
                &raw
            };
            inspection.set(key, raw, secure_input)?;
            body["operation"] = json!("set");
            body["key"] = json!(key);
            body["restart_required"] = json!(true);
        }
        ConfigCommand::Unset { key } => {
            inspection.unset(key)?;
            body["operation"] = json!("unset");
            body["key"] = json!(key);
            body["restart_required"] = json!(true);
        }
    }
    Ok((body, 0))
}

/// Config errors follow CLI exit-code conventions and never contain setting values.
pub async fn run(args: &ConfigArgs) -> i32 {
    let (body, code) = execute(args).await.unwrap_or_else(|e| {
        (
            json!({"schema_version":1,"ok":false,"error":{"code":e.code,"message":e.message}}),
            e.code,
        )
    });
    if args.json {
        println!(
            "{}",
            serde_json::to_string(&body).expect("report serializes")
        );
    } else if code != 0 {
        if let Some(message) = body.pointer("/error/message").and_then(Value::as_str) {
            eprintln!("{message}");
        } else {
            println!(
                "{}",
                serde_json::to_string_pretty(&body).expect("report serializes")
            );
        }
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&body).expect("report serializes")
        );
    }
    if body["restart_required"] == true {
        eprintln!("restart required: restart the daemon to apply runtime configuration changes");
    }
    code
}
