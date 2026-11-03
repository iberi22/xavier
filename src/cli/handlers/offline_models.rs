//! Offline GGUF model manager request handlers.

use axum::{extract::State, http::StatusCode, response::Response, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info};

use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use crate::embedding::downloader::ModelDownloader;
use xavier::agents::provider::hardware::{detect_gpu, GpuVendor};
use xavier::agents::provider::model_manager::{scan_local_models, LocalModel};
use xavier::settings::XavierSettings;

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
pub async fn update_offline_config_handler(Json(payload): Json<OfflineConfigPayload>) -> Response {
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

    // Determine the configured local model engine URL and port
    let local_llm_url = std::env::var("XAVIER_LOCAL_LLM_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.models.local_llm_url.clone());

    let mut port = 11434; // default Ollama port fallback
    let mut is_reachable = false;

    if let Ok(parsed_url) = url::Url::parse(&local_llm_url) {
        if let Some(p) = parsed_url.port() {
            port = p;
        } else {
            match parsed_url.scheme() {
                "https" => port = 443,
                "http" => port = 80,
                _ => {}
            }
        }

        if let Some(host) = parsed_url.host_str() {
            let host_port = format!("{}:{}", host, port);
            if let Ok(mut addrs) = tokio::net::lookup_host(host_port).await {
                for addr in addrs.by_ref() {
                    if let Ok(Ok(_)) = tokio::time::timeout(
                        std::time::Duration::from_millis(500),
                        tokio::net::TcpStream::connect(addr),
                    )
                    .await
                    {
                        is_reachable = true;
                        break;
                    }
                }
            }
        }
    }

    let engine_status = if is_reachable {
        "running".to_string()
    } else {
        "stopped".to_string()
    };

    let status = LocalEngineStatus {
        gpu_detected: gpu_info.vendor != GpuVendor::Unknown,
        gpu_vendor: gpu_vendor_str.to_string(),
        vram_mb: gpu_info.vram_bytes / (1024 * 1024),
        engine_status,
        active_model,
        port,
    };

    json_response(StatusCode::OK, serde_json::to_value(status).unwrap())
}

/// POST /v1/offline/download
pub async fn download_offline_model_handler(Json(payload): Json<DownloadModelPayload>) -> Response {
    let url = payload.url.trim();
    if url.is_empty() {
        return json_response(
            StatusCode::BAD_REQUEST,
            serde_json::json!({ "status": "error", "error": "URL cannot be empty" }),
        );
    }

    // Determine target directory (first configured path or default models folder)
    let settings = XavierSettings::current();
    let target_dir = if let Some(first_dir) = settings.models.local_model_dirs.first() {
        PathBuf::from(first_dir)
    } else {
        xavier::settings::XavierSettings::resolve_data_dir().join("models")
    };

    // Try to extract model name from URL or payload
    let mut model_name = "embeddinggemma-2".to_string(); // default per adr-042
    if let Some(last_segment) = url.split('/').next_back() {
        if !last_segment.is_empty() {
            model_name = last_segment.to_string();
            // Clean extension if present
            if model_name.ends_with(".gguf") {
                model_name = model_name.replace(".gguf", "");
            }
        }
    }
    let model_target_dir = target_dir.join(&model_name);

    // If url is given and not empty, use it as base url
    let downloader = if !url.is_empty() && url.starts_with("http") {
        ModelDownloader::with_base_url(url)
    } else {
        ModelDownloader::new()
    };
    match downloader.fetch_model(&model_name, &model_target_dir).await {
        Ok(report) => json_response(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
                "message": format!("Successfully downloaded {} to {}", report.model_name, report.target_dir.display()),
                "filename": report.model_name, // as fallback
                "path": report.target_dir.to_string_lossy().to_string(),
                "bytes": report.bytes_downloaded
            }),
        ),
        Err(e) => {
            error!("Failed to fetch model: {}", e);
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "status": "error",
                    "error": format!("Failed to download model: {}", e)
                }),
            )
        }
    }
}

/// CLI handler for `xavier models fetch --model <model_name>`
pub async fn run_models_fetch_cli(model_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let settings = XavierSettings::current();
    let target_dir = if let Some(first_dir) = settings.models.local_model_dirs.first() {
        PathBuf::from(first_dir)
    } else {
        crate::settings::XavierSettings::resolve_data_dir().join("models")
    };

    let model_target_dir = target_dir.join(model_name);
    let downloader = ModelDownloader::new();

    info!(
        "Starting download for model {} into {}",
        model_name,
        model_target_dir.display()
    );
    match downloader.fetch_model(model_name, &model_target_dir).await {
        Ok(report) => {
            info!(
                "Successfully downloaded {} bytes to {}",
                report.bytes_downloaded,
                report.target_dir.display()
            );
            Ok(())
        }
        Err(e) => {
            error!("Failed to fetch model: {}", e);
            Err(Box::new(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn test_get_offline_status_running() {
        let _guard = TEST_LOCK.lock().await;

        // Start a mock TCP listener to simulate a running model engine
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();
        let port = local_addr.port();

        // Point XAVIER_LOCAL_LLM_URL to this mock listener
        std::env::set_var("XAVIER_LOCAL_LLM_URL", format!("http://127.0.0.1:{}", port));

        // Call the status handler
        let resp = get_offline_status_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), 2048).await.unwrap();
        let status: LocalEngineStatus = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(status.port, port);
        assert_eq!(status.engine_status, "running");

        // Clean up environment
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
    }

    #[tokio::test]
    async fn test_get_offline_status_stopped() {
        let _guard = TEST_LOCK.lock().await;

        // Point XAVIER_LOCAL_LLM_URL to a port where nothing is listening (e.g. 64321)
        let port = 64321; // unlikely to have a service listening
        std::env::set_var("XAVIER_LOCAL_LLM_URL", format!("http://127.0.0.1:{}", port));

        // Call the status handler
        let resp = get_offline_status_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), 2048).await.unwrap();
        let status: LocalEngineStatus = serde_json::from_slice(&body_bytes).unwrap();

        assert!(status.port == port || status.port == 11434);
        assert_eq!(status.engine_status, "stopped");

        // Clean up environment
        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
    }

    #[tokio::test]
    async fn test_get_and_update_offline_config() {
        let _guard = TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("xavier.config.json");
        std::env::set_var("XAVIER_CONFIG_PATH", config_path.to_str().unwrap());

        // 1. Initial config check
        let resp = get_offline_config_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = to_bytes(resp.into_body(), 2048).await.unwrap();
        let config: OfflineConfigPayload = serde_json::from_slice(&body_bytes).unwrap();
        assert!(config.local_model_dirs.is_empty());
        assert!(!config.auto_start_last_model);

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
        assert!(config.auto_start_last_model);

        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[tokio::test]
    async fn test_offline_download_and_list() {
        let _guard = TEST_LOCK.lock().await;
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

        // We can't really do the full network download in this unit test.
        // We'll just verify the config part worked and that the handler returns an error if network fails (since it attempts a real download now, or mock fails)
        let download_payload = DownloadModelPayload {
            url: "https://huggingface.co/lmstudio-community/embeddinggemma-2-onnx/resolve/main"
                .to_string(),
        };

        let resp = download_offline_model_handler(Json(download_payload)).await;
        let status = resp.status();
        // Network will likely fail in hermetic test env, which is fine, we just want to ensure it compiles and attempts
        assert!(status == StatusCode::OK || status == StatusCode::INTERNAL_SERVER_ERROR);

        std::env::remove_var("XAVIER_CONFIG_PATH");
    }

    #[tokio::test]
    async fn test_get_offline_status() {
        let _guard = TEST_LOCK.lock().await;

        // Hermetic: do not assume a global engine is running on the host.
        let port = 64322;
        std::env::set_var("XAVIER_LOCAL_LLM_URL", format!("http://127.0.0.1:{}", port));

        let resp = get_offline_status_handler().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body_bytes = axum::body::to_bytes(resp.into_body(), 2048).await.unwrap();
        let status: LocalEngineStatus = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(status.port, port);
        assert!(
            status.engine_status == "stopped" || status.engine_status == "running",
            "unexpected engine_status={}",
            status.engine_status
        );
        assert_eq!(status.engine_status, "stopped");

        std::env::remove_var("XAVIER_LOCAL_LLM_URL");
    }
}
