#[tauri::command]
fn get_xavier_token() -> Result<String, String> {
    if let Ok(token) = std::env::var("XAVIER_TOKEN") {
        return Ok(token);
    }
    
    // Attempt to read from config file
    if let Some(mut home) = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from)) {
        home.push(".xavier");
        home.push("config");
        home.push("xavier.config.json");
        
        if let Ok(contents) = std::fs::read_to_string(home) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(token) = json.get("security").and_then(|s| s.get("token_secret")).and_then(|t| t.as_str()) {
                    return Ok(token.to_string());
                }
            }
        }
    }
    Err("Token not found in environment or config file".to_string())
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

            let (mut _rx, _child) = sidecar_command
                .args(["http"])
                .spawn()
                .map_err(|e| {
                    log::error!("Failed to spawn xavier sidecar: {}", e);
                    e
                })?;

            log::info!("Xavier sidecar spawned successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_xavier_token])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
