use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::Command as StdCommand;
use sysinfo::System;
use tauri::{
    menu::{Menu, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_shell::ShellExt;
use xavier::ui_logger::{log_ui_error, log_ui_info, UILogLevel, log_ui_event};

// ── Constants ──────────────────────────────────────────────────
const LOCK_FILENAME: &str = "xavier.lock";

// ── UI Logging commands ────────────────────────────────────────

#[tauri::command]
async fn log_ui_event_cmd(
    level: String,
    component: String,
    message: String,
    context: Option<serde_json::Value>,
    stack_trace: Option<String>,
) -> Result<(), String> {
    let log_level = match level.as_str() {
        "debug" => UILogLevel::Debug,
        "info" => UILogLevel::Info,
        "warning" => UILogLevel::Warning,
        "error" => UILogLevel::Error,
        "critical" => UILogLevel::Critical,
        _ => UILogLevel::Info,
    };

    log_ui_event(log_level, &component, &message, context, stack_trace)
        .await
        .map_err(|e| e.to_string())
}

// ── Xavier token ───────────────────────────────────────────────

#[tauri::command]
fn get_xavier_token() -> Result<String, String> {
    if let Ok(token) = std::env::var("XAVIER_TOKEN") {
        return Ok(token);
    }

    // Read from config file
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

// ── System scan command ────────────────────────────────────────

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
            .args(["path", "win32_VideoController", "get", "name"])
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

// ── Initial config command ─────────────────────────────────────

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

// ── Navigate helper ────────────────────────────────────────────

fn navigate_to_tab(app: &tauri::AppHandle, tab: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("navigate-to", tab);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

// ── Single-instance lock ───────────────────────────────────────

fn dirs_data_local_dir() -> std::path::PathBuf {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        let mut p = std::path::PathBuf::from(app_data);
        p.push("Xavier");
        return p;
    }
    std::path::PathBuf::from(".")
}

/// Checks for existing Xavier instances and kills them,
/// then writes our PID to the lock file.
/// Always returns `Ok(lock_path)` — callers should proceed normally.
fn single_instance_check() -> std::path::PathBuf {
    let lock_dir = dirs_data_local_dir();
    std::fs::create_dir_all(&lock_dir).ok();
    let lock_path = lock_dir.join(LOCK_FILENAME);
    let self_pid = std::process::id();

    // 1. Check lock file for a running PID
    if let Ok(content) = std::fs::read_to_string(&lock_path) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if pid != self_pid {
                if let Ok(output) = StdCommand::new("tasklist")
                    .args(["/FI", &format!("PID eq {}", pid), "/NH"])
                    .output()
                {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if stdout.contains(&pid.to_string()) {
                        log::info!("Lock file says PID {} is alive — killing it", pid);
                        let _ = StdCommand::new("taskkill")
                            .args(["/F", "/PID", &pid.to_string()])
                            .output();
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            }
        }
    }

    // 2. Scan for existing app.exe processes (catch stray instances)
    let mut sys = System::new();
    sys.refresh_all();
    for (proc_pid, proc) in sys.processes() {
        let pid_u32 = proc_pid.as_u32();
        if pid_u32 == self_pid {
            continue;
        }
        let name = proc.name().to_string_lossy().to_lowercase();
        if name == "app.exe" || name == "app" {
            log::info!("Found existing app.exe (PID {}) — killing it", pid_u32);
            let _ = StdCommand::new("taskkill")
                .args(["/F", "/PID", &pid_u32.to_string()])
                .output();
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }

    // 3. Write our PID
    if let Ok(mut file) = std::fs::File::create(&lock_path) {
        let _ = write!(file, "{}", self_pid);
        let _ = file.sync_all();
    }

    lock_path
}

// ── Window registration for tray events ───────────────────────

#[tauri::command]
fn register_window(_window: tauri::Window) {
    log::info!("Frontend registered for tray events");
}

// ── API Token management commands ──────────────────────────────

#[tauri::command]
async fn create_api_token(
    name: String,
    scopes: Vec<String>,
    expires_at: Option<String>,
) -> Result<serde_json::Value, String> {
    let store = xavier::security::tokens::TokenStore::new();
    let expiry = expires_at.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .ok()
            .map(|dt| dt.with_timezone(&chrono::Utc))
    });

    match store.create_token(name, scopes, expiry).await {
        Ok((plaintext, metadata)) => Ok(serde_json::json!({
            "token": plaintext,
            "metadata": metadata
        })),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn list_api_tokens() -> Result<Vec<xavier::security::tokens::ApiTokenMetadata>, String> {
    let store = xavier::security::tokens::TokenStore::new();
    match store.list_tokens().await {
        Ok(tokens) => Ok(tokens),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
async fn evoke_api_token(id: String) -> Result<(), String> {
    let store = xavier::security::tokens::TokenStore::new();
    match store.revoke_token(&id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ── Application entry point ────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ── Single-instance check: kill any existing instance first ───
    let lock_path = single_instance_check();
    log::info!("Single-instance lock acquired at {:?}", lock_path);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            // Initialize UI logger
            log::info!("Initializing Xavier Panel UI");
            
            // Log startup
            tauri::async_runtime::spawn(async {
                let _ = log_ui_info("TauriApp", "Xavier Panel UI starting up").await;
            });

            // Initialize Xavier's Tauri AppHandle
            xavier::utils::tauri_utils::set_tauri_app_handle(app.handle().clone());

            // Initialize Notification Forwarder
            xavier::notifications::NOTIFICATIONS.spawn_tauri_forwarder();

            // ── Build tray menu ─────────────────────────────────────
            let open_app = MenuItemBuilder::with_id("open_app", "Open Xavier")
                .build(app)
                .unwrap();
            let open_history = MenuItemBuilder::with_id("open_history", "Open History")
                .build(app)
                .unwrap();
            let open_graph = MenuItemBuilder::with_id("open_graph", "Open Knowledge Graph")
                .build(app)
                .unwrap();
            let open_config = MenuItemBuilder::with_id("open_config", "Open Configuration")
                .build(app)
                .unwrap();
            let open_providers = MenuItemBuilder::with_id("open_providers", "Open Providers")
                .build(app)
                .unwrap();
            let separator = tauri::menu::PredefinedMenuItem::separator(app).unwrap();
            let quit = MenuItemBuilder::with_id("quit", "Close Xavier")
                .accelerator("Alt+F4")
                .build(app)
                .unwrap();

            let menu = Menu::with_items(
                app,
                &[
                    &open_app,
                    &open_history,
                    &open_graph,
                    &open_config,
                    &open_providers,
                    &separator,
                    &quit,
                ],
            )
            .unwrap();

            // ── Build tray icon ─────────────────────────────────────
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("Xavier - Cognitive Memory System")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open_app" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "open_history" => navigate_to_tab(app, "history"),
                    "open_graph" => navigate_to_tab(app, "graph"),
                    "open_config" => navigate_to_tab(app, "config"),
                    "open_providers" => navigate_to_tab(app, "providers"),
                    "quit" => {
                        log::info!("Closing Xavier via tray menu");
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)
                .unwrap();

            // ── Spawn Xavier sidecar ────────────────────────────────
            // Xavier stores its SQLite databases in ~/.xavier/, so we must
            // set the working directory to the user's home dir so it finds them.
            let xavier_cwd = std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
                .unwrap_or_else(|| std::path::PathBuf::from("."));

            let shell = app.shell();
            let sidecar_command = shell.sidecar("xavier").map_err(|e| {
                log::error!("Failed to create sidecar command: {}", e);
                e
            })?;

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
                .env("XAVIER_TOKEN", token.clone())
                .args(["http", "8006"])
                .current_dir(&xavier_cwd)
                .spawn()
                .map_err(|e| {
                    let err_msg = format!("{}", e);
                    let err_debug = format!("{:?}", e);
                    log::error!("Failed to spawn xavier sidecar: {}", e);
                    tauri::async_runtime::spawn(async move {
                        let _ = log_ui_error(
                            "XavierSidecar",
                            &format!("Failed to spawn: {}", err_msg),
                            Some(err_debug),
                        )
                        .await;
                    });
                    e
                })?;

            log::info!(
                "Xavier sidecar spawned successfully (CWD: {:?})",
                xavier_cwd
            );
            
            // Log successful sidecar spawn
            let cwd_str = format!("{:?}", xavier_cwd);
            tauri::async_runtime::spawn(async move {
                let _ = log_ui_info(
                    "XavierSidecar",
                    &format!("Backend server started successfully at CWD: {}", cwd_str),
                )
                .await;
            });

            // Hide window on close (minimize to tray instead of quitting)
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        let _ = window_clone.hide();
                        api.prevent_close();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            log_ui_event_cmd,
            get_xavier_token,
            scan_system,
            save_initial_config,
            register_window,
            create_api_token,
            list_api_tokens,
            evoke_api_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    // Clean up lock file on exit
    let lock_dir = dirs_data_local_dir();
    let lock_path = lock_dir.join(LOCK_FILENAME);
    if lock_path.exists() {
        let _ = std::fs::remove_file(&lock_path);
    }
}
