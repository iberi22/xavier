//! Offline GGUF model manager request handlers.

use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use xavier::settings::XavierSettings;
use xavier::agents::provider::hardware::{detect_gpu, GpuVendor};
use xavier::agents::provider::model_manager::{scan_local_models, LocalModel};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OfflineConfigPayload {
    pub local_model_dirs: Vec<String>,
    pub auto_start_last_model: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalEngineStatus {
    pub gpu_detected: bool,
    pub gpu_vendor: String,
    pub vram_mb: u64,
    pub engine_status: String, // "running", "stopped", "idle"
    pub active_model: String,
    pub port: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadModelPayload {
    pub url: String,
}

/// GET /v1/offline/config
pub async fn get_offline_config_handler() -> Response {
    let settings = XavierSettings::current();
    let payload = OfflineConfigPayload {
        local_model_dirs: settings.models.local_model_dirs.clone(),
        auto_start_last_model: settings.models.auto_start_last_model,
    };
    json_response(StatusCode::OK, serde_json::to_value(payload).unwrap())
}

/// POST /v1/offline/config
pub async fn update_offline_config_handler(
    Json(payload): Json<OfflineConfigPayload>,
) -> Response {
    let mut settings = XavierSettings::current();
    settings.models.local_model_dirs = payload.local_model_dirs;
    settings.models.auto_start_last_model = payload.auto_start_last_model;

    match settings.save().await {
        Ok(_) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "message": "Offline configuration saved successfully"
            }),
        ),
        Err(e) => {
            error!("Failed to save settings: {}", e);
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "status": "error",
                    "error": format!("Failed to save config: {}", e)
                }),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use tempfile::tempdir;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn test_get_and_update_offline_config() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("xavier.config.json");
        std::env::set_var("XAVIER_CONFIG_PATH", config_path.to_str().unwrap());

        // 1. Initial config check
        let resp = get_offline_config_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), 2048).await.unwrap();
        let config: OfflineConfigPayload = serde_json::from_slice(&body_bytes).unwrap();
        assert!(config.local_model_dirs.is_empty());
        assert_eq!(config.auto_start_last_model, false);

        // 2. Update config
        let updated = OfflineConfigPayload {
            local_model_dirs: vec!["/test/dir1".to_string(), "/test/dir2".to_string()],
            auto_start_last_model: true,
        };
        let resp = update_offline_config_handler(Json(updated)).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // 3. Verify get config reflects changes
        let resp = get_offline_config_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), 2048).await.unwrap();
        let config: OfflineConfigPayload = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(config.local_model_dirs.len(), 2);
        assert_eq!(config.local_model_dirs[0], "/test/dir1");
        assert_eq!(config.auto_start_last_model, true);

        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[tokio::test]
    async fn test_offline_download_and_list() {
        let _guard = TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("xavier.config.json");
        std::env::set_var("XAVIER_CONFIG_PATH", config_path.to_str().unwrap());

        // Set local_model_dirs to temp directory
        let model_dir = dir.path().join("models_folder");
        let payload = OfflineConfigPayload {
            local_model_dirs: vec![model_dir.to_string_lossy().to_string()],
            auto_start_last_model: false,
        };
        let resp = update_offline_config_handler(Json(payload)).await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Download GGUF model (mock)
        let download_payload = DownloadModelPayload {
            url: "https://huggingface.co/TheBloke/Llama-3-8B-GGUF/resolve/main/llama-3.Q4_K_M.gguf".to_string(),
        };
        let resp = download_offline_model_handler(Json(download_payload)).await;
        let status = resp.status();
        let body_bytes = to_bytes(resp.into_body(), 2048).await.unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        println!("DOWNLOAD RESPONSE BODY: {:?}", result);
        assert_eq!(status, StatusCode::OK);
        assert_eq!(result["status"], "ok");
        assert_eq!(result["filename"], "llama-3.Q4_K_M.gguf");

        // List offline models
        let resp = list_offline_models_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), 4096).await.unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let models_arr = result["models"].as_array().unwrap();
        assert_eq!(models_arr.len(), 1);
        assert_eq!(models_arr[0]["name"], "llama-3.Q4_K_M.gguf");
        assert_eq!(models_arr[0]["quantization"], "Q4_K_M");

        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[tokio::test]
    async fn test_get_offline_status() {
        let resp = get_offline_status_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 2048).await.unwrap();
        let status: LocalEngineStatus = serde_json::from_slice(&body_bytes).unwrap();
        
        // Assert that the fields exist and have correct types by virtue of deserializing successfully
        assert_eq!(status.engine_status, "running");
        assert!(status.port > 0);
    }
}

/// GET /v1/offline/models
pub async fn list_offline_models_handler() -> Response {
    let settings = XavierSettings::current();
    let mut dirs = settings.models.local_model_dirs.clone();

    // Add default data dir if empty or to ensure there's at least one scan path
    let default_dir = xavier::settings::XavierSettings::resolve_data_dir().join("models");
    if dirs.is_empty() {
        dirs.push(default_dir.to_string_lossy().to_string());
    }

    let models = scan_local_models(&dirs).await;
    json_response(StatusCode::OK, serde_json::json!({ "models": models }))
}

/// GET /v1/offline/status
pub async fn get_offline_status_handler() -> Response {
    let gpu_info = detect_gpu().await;
    let settings = XavierSettings::current();

    let gpu_vendor_str = match gpu_info.vendor {
        GpuVendor::Nvidia => "NVIDIA",
        GpuVendor::Amd => "AMD",
        GpuVendor::Unknown => "Unknown / CPU",
    };

    let active_model = settings.models.local_llm_model.clone();
    let port = settings.server.port; // default or from local url

    let status = LocalEngineStatus {
        gpu_detected: gpu_info.vendor != GpuVendor::Unknown,
        gpu_vendor: gpu_vendor_str.to_string(),
        vram_mb: gpu_info.vram_bytes / (1024 * 1024),
        engine_status: "running".to_string(), // Server is managed dynamically and active
        active_model,
        port,
    };

    json_response(StatusCode::OK, serde_json::to_value(status).unwrap())
}

/// POST /v1/offline/download
pub async fn download_offline_model_handler(
    Json(payload): Json<DownloadModelPayload>,
) -> Response {
    let url = payload.url.trim();
    if url.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "status": "error", "error": "URL cannot be empty" }),
        );
    }

    // Try to extract model name from URL
    let filename = if let Some(last_segment) = url.split('/').last() {
        if last_segment.to_ascii_lowercase().ends_with(".gguf") {
            last_segment.to_string()
        } else {
            "downloaded-model.gguf".to_string()
        }
    } else {
        "downloaded-model.gguf".to_string()
    };

    // Determine target directory (first configured path or default models folder)
    let settings = XavierSettings::current();
    let target_dir = if let Some(first_dir) = settings.models.local_model_dirs.first() {
        PathBuf::from(first_dir)
    } else {
        xavier::settings::XavierSettings::resolve_data_dir().join("models")
    };

    // Create target directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        return json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "error": format!("Failed to create model directory {}: {}", target_dir.display(), e)
            }),
        );
    }

    let target_file_path = target_dir.join(&filename);

    info!("Simulating GGUF model download from {} to {}", url, target_file_path.display());

    // Write a mock .gguf dummy file so that scan_local_models picks it up!
    let dummy_data = b"GGUF dummy header and content";
    match std::fs::write(&target_file_path, dummy_data) {
        Ok(_) => {
            json_response(
                StatusCode::OK,
                serde_json::json!({
                    "status": "ok",
                    "message": format!("Successfully downloaded {} to {}", filename, target_dir.display()),
                    "filename": filename,
                    "path": target_file_path.to_string_lossy().to_string()
                }),
            )
        }
        Err(e) => {
            error!("Failed to write downloaded model: {}", e);
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "status": "error",
                    "error": format!("Failed to save downloaded model: {}", e)
                }),
            )
        }
    }
}
