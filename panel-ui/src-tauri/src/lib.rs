#[tauri::command]
fn get_xavier_token() -> Result<String, String> {
    if let Ok(token) = std::env::var("XAVIER_TOKEN") {
        return Ok(token);
    }

    // Attempt to read from config file
    if let Some(mut home) = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
    {
        home.push(".xavier");
        home.push("config");
        home.push("xavier.config.json");

        if let Ok(contents) = std::fs::read_to_string(home) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(token) = json
                    .get("security")
                    .and_then(|s| s.get("token_secret"))
                    .and_then(|t| t.as_str())
                {
                    return Ok(token.to_string());
                }
            }
        }
    }
    Err("Token not found in environment or config file".to_string())
}

use serde::{Deserialize, Serialize};
use std::process::Command as StdCommand;
use sysinfo::System;

#[derive(Serialize)]
struct SystemInfo {
    total_ram_gb: f64,
    cpu_cores: usize,
    has_gpu: bool,
    openclaw_running: bool,
    hermes_running: bool,
}

#[tauri::command]
fn scan_system() -> Result<SystemInfo, String> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let total_ram_gb = sys.total_memory() as f64 / 1_073_741_824.0;
    let cpu_cores = sys.cpus().len();

    let mut openclaw_running = false;
    let mut hermes_running = false;

    for process in sys.processes().values() {
        let name = process.name().to_string_lossy().to_lowercase();
        if name.contains("openclaw") {
            openclaw_running = true;
        }
        if name.contains("hermes") {
            hermes_running = true;
        }
    }

    let has_gpu = if cfg!(target_os = "windows") {
        let output = StdCommand::new("wmic")
            .args(&["path", "win32_VideoController", "get", "name"])
            .output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
            stdout.contains("nvidia")
                || stdout.contains("amd")
                || stdout.contains("radeon")
                || stdout.contains("rtx")
                || stdout.contains("gtx")
        } else {
            false
        }
    } else {
        false
    };

    Ok(SystemInfo {
        total_ram_gb,
        cpu_cores,
        has_gpu,
        openclaw_running,
        hermes_running,
    })
}

#[derive(Deserialize)]
struct InitialConfig {
    telegram_token: Option<String>,
    use_gpu_model: bool,
}

#[tauri::command]
fn save_initial_config(config: InitialConfig) -> Result<(), String> {
    let mut home = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .ok_or("Home dir not found")?;
    home.push(".xavier");
    home.push("config");

    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;

    home.push("xavier.config.json");

    let mut json = serde_json::json!({});

    if home.exists() {
        if let Ok(contents) = std::fs::read_to_string(&home) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&contents) {
                json = parsed;
            }
        }
    }

    if let Some(token) = config.telegram_token {
        if !token.is_empty() {
            json["telegram"] = serde_json::json!({
                "bot_token": token,
                "enabled": true
            });
        }
    }

    let model_settings = if config.use_gpu_model {
        serde_json::json!({
            "local_llm_model": "gpu-fast-model",
            "embedding_model": "nomic-embed-text"
        })
    } else {
        serde_json::json!({
            "local_llm_model": "cpu-fast-model",
            "embedding_model": "nomic-embed-text"
        })
    };
    json["models"] = model_settings;

    let updated_json = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(home, updated_json).map_err(|e| e.to_string())?;

    Ok(())
}
use tauri_plugin_shell::ShellExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            let shell = app.shell();
            let sidecar_command = shell.sidecar("xavier").map_err(|e| {
                log::error!("Failed to create sidecar command: {}", e);
                e
            })?;

            // Generate or fetch XAVIER_TOKEN
            let token = match get_xavier_token() {
                Ok(t) => t,
                Err(_) => {
                    let new_token = uuid::Uuid::new_v4().to_string();
                    if let Some(mut home) = std::env::var_os("USERPROFILE")
                        .map(std::path::PathBuf::from)
                        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
                    {
                        home.push(".xavier");
                        home.push("config");
                        let _ = std::fs::create_dir_all(&home);
                        home.push("xavier.config.json");

                        let mut json = serde_json::json!({});
                        if let Ok(contents) = std::fs::read_to_string(&home) {
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&contents)
                            {
                                json = parsed;
                            }
                        }
                        json["security"] = serde_json::json!({ "token_secret": new_token });
                        if let Ok(updated_json) = serde_json::to_string_pretty(&json) {
                            let _ = std::fs::write(home, updated_json);
                        }
                    }
                    new_token
                }
            };

            let (mut _rx, _child) = sidecar_command
                .env("XAVIER_TOKEN", token)
                .args(["http"])
                .spawn()
                .map_err(|e| {
                    log::error!("Failed to spawn xavier sidecar: {}", e);
                    e
                })?;

            log::info!("Xavier sidecar spawned successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_xavier_token,
            scan_system,
            save_initial_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
