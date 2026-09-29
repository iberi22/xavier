//! Xavier desktop shell (Tauri 2).
//!
//! Safety contract: the shell never stops a Xavier it did not start. On launch
//! it probes `/health`; if a server answers (e.g. the `xavier.service` systemd
//! unit) the UI attaches to it and no sidecar is spawned. Only when nothing
//! answers is the bundled `xavier http` sidecar started, and on exit only that
//! child is terminated. Authentication is the panel's own `/auth/*` login.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::Command as StdCommand;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use sysinfo::System;
use tauri::{
    menu::{Menu, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_shell::process::CommandChild;
use tauri_plugin_shell::ShellExt;

const DEFAULT_PORT: u16 = 8006;
const HEALTH_POLL_ATTEMPTS: u32 = 60;

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
    } else if cfg!(target_os = "linux") {
        // Linux GPU detection: try lspci, fall back to /proc and /sys
        let mut detected = false;
        if let Ok(output) = StdCommand::new("sh")
            .arg("-c")
            .arg("lspci 2>/dev/null | grep -iE 'vga|3d|display' || true")
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
            detected = stdout.contains("nvidia")
                || stdout.contains("amd")
                || stdout.contains("radeon")
                || stdout.contains("intel");
        }
        if !detected {
            detected = std::path::Path::new("/dev/dri/renderD128").exists()
                || std::path::Path::new("/sys/class/drm").exists();
        }
        detected
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
    // XDG-aware: XDG_CONFIG_HOME > $HOME/.config > USERPROFILE (Windows)
    let mut config_dir = if let Some(xdg) =
        std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
    {
        xdg
    } else if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        let mut p = home;
        p.push(".config");
        p
    } else if let Some(userprofile) = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from)
    {
        userprofile
    } else {
        return Err("Home dir not found".to_string());
    };
    config_dir.push("xavier");

    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    let mut home = config_dir;
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

fn show_main(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

// ── Sidecar state (only ever holds a child WE spawned) ─────────

#[derive(Clone, Default)]
pub struct SidecarState(Arc<Mutex<Option<CommandChild>>>);

impl SidecarState {
    fn set_child(&self, child: CommandChild) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(child);
        }
    }

    /// Terminates the sidecar this app spawned; no-op when attached to an
    /// externally managed Xavier.
    fn kill(&self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(child) = guard.take() {
                match child.kill() {
                    Ok(()) => log::info!("Xavier sidecar stopped"),
                    Err(e) => log::error!("Failed to stop sidecar: {e}"),
                }
            }
        }
    }
}

// ── Server discovery ───────────────────────────────────────────

fn configured_port() -> u16 {
    std::env::var("XAVIER_PORT")
        .ok()
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Validates a raw HTTP response from `GET /health`.
/// Accepts only HTTP 200 responses with valid JSON where `service == "xavier"`
/// and `status` is a string (e.g. "healthy", "degraded", "warn").
fn is_xavier_health_response(text: &str) -> bool {
    let ok_status = text.lines().next().is_some_and(|l| l.contains(" 200"));
    if !ok_status {
        return false;
    }
    let body = if let Some((_, b)) = text.split_once("\r\n\r\n") {
        b
    } else if let Some((_, b)) = text.split_once("\n\n") {
        b
    } else {
        return false;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body.trim()) else {
        return false;
    };
    json.get("service").and_then(|v| v.as_str()) == Some("xavier")
        && json.get("status").and_then(|v| v.as_str()).is_some()
}

/// Minimal dependency-free `GET /health`; true when a Xavier-shaped answer
/// (HTTP 200 with service == "xavier" and a JSON `status`) comes back.
fn probe_health(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(800)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let req = "GET /health HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    let _ = stream.take(16 * 1024).read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    is_xavier_health_response(&text)
}

fn port_is_bound(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
}

/// Data dir private to the desktop shell (never the production `~/.xavier`).
fn desktop_data_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("XAVIER_DESKTOP_DATA_DIR") {
        return dir.into();
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("xavier-desktop");
    }
    if cfg!(target_os = "windows") {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return std::path::PathBuf::from(app_data).join("xavier-desktop");
        }
    }
    home_dir()
        .unwrap_or_else(|| ".".into())
        .join(".local")
        .join("share")
        .join("xavier-desktop")
}

/// Random token for the sidecar's root API token, persisted (0600) in the
/// desktop data dir so it is stable across launches.
fn sidecar_token(data_dir: &std::path::Path) -> String {
    let path = data_dir.join("sidecar-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        if !t.trim().is_empty() {
            return t.trim().to_string();
        }
    }
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    if std::fs::write(&path, &token).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
    token
}

enum Backend {
    /// A Xavier we do not own answered; nothing was spawned.
    Attached,
    /// We started the bundled sidecar.
    Spawned,
    /// Could not attach or spawn (reason for the UI).
    Unavailable(String),
}

fn start_backend(app: &tauri::App, port: u16, state: &SidecarState) -> Backend {
    if probe_health(port) {
        log::info!("Xavier already running on port {port}: attaching (no sidecar)");
        return Backend::Attached;
    }
    if port_is_bound(port) {
        return Backend::Unavailable(format!(
            "Port {port} is in use by another application that is not Xavier."
        ));
    }

    let data_dir = desktop_data_dir();
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        return Backend::Unavailable(format!("Cannot create {}: {e}", data_dir.display()));
    }
    let token = sidecar_token(&data_dir);

    let mut cmd = match app.shell().sidecar("xavier") {
        Ok(cmd) => cmd,
        Err(e) => return Backend::Unavailable(format!("Bundled Xavier not found: {e}")),
    };
    cmd = cmd
        .env("XAVIER_TOKEN", token)
        .env("XAVIER_DATA_DIR", &data_dir)
        .env("XAVIER_HOME", &data_dir)
        .env("XAVIER_MEMORY_VEC_PATH", data_dir.join("vec-store.sqlite3"))
        .env("XAVIER_PORT", port.to_string())
        .current_dir(&data_dir);

    // Optional bundled libpdfium for PageIndex PDF layout (bookmarks work without).
    if let Ok(res) = app.path().resource_dir() {
        let lib = res.join("resources").join("libpdfium.so");
        if lib.exists() && std::env::var_os("XAVIER_PAGEINDEX_PDFIUM_LIB").is_none() {
            cmd = cmd.env("XAVIER_PAGEINDEX_PDFIUM_LIB", lib);
        }
    }

    // MCP HTTP is disabled so the shell never contends for the MCP port.
    match cmd
        .args(["http", &port.to_string(), "--mcp-port", "0"])
        .spawn()
    {
        Ok((_rx, child)) => {
            state.set_child(child);
            log::info!(
                "Xavier sidecar spawned on port {port} (data: {})",
                data_dir.display()
            );
            Backend::Spawned
        }
        Err(e) => Backend::Unavailable(format!("Failed to start bundled Xavier: {e}")),
    }
}

// ── Application entry point ────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .setup(|app| {
            let sidecar_state = SidecarState::default();
            app.manage(sidecar_state.clone());

            let port = configured_port();
            let backend = start_backend(app, port, &sidecar_state);
            let base = format!("http://127.0.0.1:{port}");

            // Point the panel at the resolved server. Only overwrite a value we
            // manage (loopback) so a user-chosen remote URL is preserved.
            let init = format!(
                "try{{var k='xavier_remote_url',v=localStorage.getItem(k);\
                 if(!v||v.indexOf('http://127.0.0.1:')===0)localStorage.setItem(k,'{base}');}}catch(e){{}}"
            );
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Xavier")
                .inner_size(1280.0, 800.0)
                .min_inner_size(900.0, 600.0)
                .initialization_script(&init)
                .on_page_load(|window, payload| {
                    log::info!(
                        "Webview page {:?}: {} ({})",
                        payload.event(),
                        payload.url(),
                        window.label()
                    );
                })
                .build()?;
            log::info!("Main window created");

            // Closing the window quits (and stops only our own sidecar).
            // Tray is best-effort: GNOME etc. may have no indicator host.
            // The indicator library is dlopen'd and panics when missing, so
            // treat a panic like an error: the window must still open.
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_tray(app))) {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("System tray unavailable: {e}"),
                Err(_) => log::warn!("System tray unavailable: indicator library missing"),
            }

            let app_handle = app.handle().clone();
            let (status, message) = match &backend {
                Backend::Attached => ("ready", format!("Attached to Xavier on port {port}")),
                Backend::Spawned => ("ready", format!("Bundled Xavier starting on port {port}")),
                Backend::Unavailable(reason) => ("error", reason.clone()),
            };
            if matches!(backend, Backend::Unavailable(_)) {
                log::error!("Backend unavailable: {message}");
                let _ = app_handle.emit(
                    "backend-health-status",
                    serde_json::json!({ "status": status, "message": message }),
                );
                return Ok(());
            }

            tauri::async_runtime::spawn_blocking(move || {
                for attempt in 1..=HEALTH_POLL_ATTEMPTS {
                    if probe_health(port) {
                        log::info!("Backend healthy on attempt {attempt}");
                        let _ = app_handle.emit(
                            "backend-health-status",
                            serde_json::json!({ "status": "ready", "message": message }),
                        );
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                let msg = format!("Xavier did not answer /health on port {port}.");
                log::error!("{msg}");
                let _ = app_handle.emit(
                    "backend-health-status",
                    serde_json::json!({ "status": "error", "message": msg }),
                );
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_system,
            save_initial_config,
            register_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<SidecarState>() {
                state.kill();
            }
        }
    });
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open_app = MenuItemBuilder::with_id("open_app", "Open Xavier").build(app)?;
    let open_history = MenuItemBuilder::with_id("open_history", "Open History").build(app)?;
    let open_graph = MenuItemBuilder::with_id("open_graph", "Open Knowledge Graph").build(app)?;
    let open_config = MenuItemBuilder::with_id("open_config", "Open Configuration").build(app)?;
    let open_providers = MenuItemBuilder::with_id("open_providers", "Open Providers").build(app)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Xavier").build(app)?;
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
    )?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Xavier - Cognitive Memory System")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open_app" => show_main(app),
            "open_history" => navigate_to_tab(app, "history"),
            "open_graph" => navigate_to_tab(app, "graph"),
            "open_config" => navigate_to_tab(app, "config"),
            "open_providers" => navigate_to_tab(app, "providers"),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[tauri::command]
fn register_window(_window: tauri::Window) {
    log::info!("Frontend registered for tray events");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xavier_health_response_accept() {
        let valid_crlf = "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"status\":\"healthy\",\"service\":\"xavier\",\"version\":\"0.2.16\"}";
        assert!(is_xavier_health_response(valid_crlf));

        let valid_lf = "HTTP/1.1 200 OK\nContent-Type: application/json\n\n{\"status\":\"degraded\",\"service\":\"xavier\"}";
        assert!(is_xavier_health_response(valid_lf));
    }

    #[test]
    fn test_xavier_health_response_reject() {
        // Non-200 status
        let status_500 = "HTTP/1.0 500 Internal Server Error\r\n\r\n{\"status\":\"unhealthy\",\"service\":\"xavier\"}";
        assert!(!is_xavier_health_response(status_500));

        // Missing service identifier
        let no_service = "HTTP/1.0 200 OK\r\n\r\n{\"status\":\"healthy\"}";
        assert!(!is_xavier_health_response(no_service));

        // Unrelated service
        let other_service = "HTTP/1.0 200 OK\r\n\r\n{\"status\":\"healthy\",\"service\":\"nginx\"}";
        assert!(!is_xavier_health_response(other_service));

        // Missing status field
        let no_status = "HTTP/1.0 200 OK\r\n\r\n{\"service\":\"xavier\"}";
        assert!(!is_xavier_health_response(no_status));

        // Non-JSON body
        let html_body = "HTTP/1.0 200 OK\r\n\r\n<html><body>status: ok</body></html>";
        assert!(!is_xavier_health_response(html_body));

        // Empty body or malformed headers
        assert!(!is_xavier_health_response("HTTP/1.0 200 OK"));
        assert!(!is_xavier_health_response(""));
    }
}
