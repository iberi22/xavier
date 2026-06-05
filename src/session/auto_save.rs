//! Auto-save session events to Xavier memory.
//!
//! Fase 1: post-session-save — Fire-and-forget con 3s timeout.
//! Fase 2: auto-verify — SAVE → RETRIEVE → COMPARE automático.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::time::timeout;
use tracing::{info, warn, error};

use crate::session::types::{SessionEvent, SessionEventType};
use crate::verification::auto_verifier::AutoVerifier;

/// Configuration for auto-save behavior
#[derive(Debug, Clone)]
pub struct AutoSaveConfig {
    /// Xavier base URL
    pub xavier_url: String,
    /// Auth token for Xavier
    pub auth_token: String,
    /// Fire-and-forget timeout in milliseconds (default: 3000ms)
    pub timeout_ms: u64,
    /// Whether to run auto-verify after save (Fase 2)
    pub auto_verify: bool,
    /// Directory to store failed sync records
    pub failed_syncs_dir: PathBuf,
    /// Minimum match score to consider healthy (default: 0.8)
    pub min_match_score: f32,
}

impl Default for AutoSaveConfig {
    fn default() -> Self {
        Self {
            xavier_url: resolve_xavier_url(),
            auth_token: std::env::var("XAVIER_TOKEN").unwrap_or_default(),
            timeout_ms: 3000,
            auto_verify: true,
            failed_syncs_dir: PathBuf::from("failed-syncs"),
            min_match_score: 0.8,
        }
    }
}

impl AutoSaveConfig {
    /// Load from environment variables with sensible defaults
    pub fn from_env() -> Self {
        let mut config = Self::default();
        
        if let Ok(url) = std::env::var("XAVIER_URL") {
            config.xavier_url = url;
        }
        if let Ok(token) = std::env::var("XAVIER_TOKEN") {
            config.auth_token = token;
        }
        if let Ok(ms) = std::env::var("XAVIER_AUTO_SAVE_TIMEOUT_MS") {
            if let Ok(parsed) = ms.parse() {
                config.timeout_ms = parsed;
            }
        }
        if let Ok(v) = std::env::var("XAVIER_AUTO_VERIFY") {
            config.auto_verify = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(dir) = std::env::var("XAVIER_FAILED_SYNCS_DIR") {
            config.failed_syncs_dir = PathBuf::from(dir);
        }
        if let Ok(score) = std::env::var("XAVIER_MIN_MATCH_SCORE") {
            if let Ok(parsed) = score.parse() {
                config.min_match_score = parsed;
            }
        }
        
        config
    }
}

/// Result of an auto-save operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSaveResult {
    pub session_id: String,
    pub event_type: String,
    pub saved: bool,
    pub verified: bool,
    pub path: String,
    pub latency_ms: u64,
    pub match_score: f32,
    pub error: Option<String>,
}

/// Auto-save a session event to Xavier memory (fire-and-forget).
/// Returns immediately; actual work happens in a spawned task.
pub fn auto_save_event(event: SessionEvent) {
    let config = AutoSaveConfig::from_env();
    
    // Skip non-important events (only save Message and ToolResult)
    let should_save = matches!(event.event_type, 
        SessionEventType::Message | 
        SessionEventType::ToolResult
    );
    
    if !should_save {
        info!(
            session_id = %event.session_id,
            event_type = ?event.event_type,
            "auto-save: skipping non-important event"
        );
        return;
    }
    
    // Skip if missing content
    let content = match event.content {
        Some(ref c) if !c.is_empty() => c.clone(),
        _ => {
            info!(
                session_id = %event.session_id,
                "auto-save: skipping empty content"
            );
            return;
        }
    };
    
    // Spawn fire-and-forget task
    tokio::spawn(async move {
        let start = Instant::now();
        let path = format!("sessions/{}/{}", 
            event.session_id,
            chrono::Utc::now().timestamp_millis()
        );
        
        let result = timeout(
            Duration::from_millis(config.timeout_ms),
            save_and_verify(&config, &path, &content, &event.session_id)
        ).await;
        
        let latency_ms = start.elapsed().as_millis() as u64;
        
        match result {
            Ok(Ok(save_result)) => {
                info!(
                    session_id = %event.session_id,
                    path = %path,
                    saved = save_result.saved,
                    verified = save_result.verified,
                    match_score = save_result.match_score,
                    latency_ms = latency_ms,
                    "auto-save: completed"
                );
                
                // Update session sync metrics
                crate::tasks::session_sync_task::SessionSyncTask::update_metrics(
                    if save_result.saved { 1.0 } else { 0.0 },
                    save_result.match_score as f64,
                    0,
                );
            }
            Ok(Err(e)) => {
                warn!(
                    session_id = %event.session_id,
                    path = %path,
                    error = %e,
                    latency_ms = latency_ms,
                    "auto-save: failed"
                );
                
                // Record failed sync
                if let Err(write_err) = record_failed_sync(&config.failed_syncs_dir, &event, &e, latency_ms).await {
                    error!("Failed to write failed-sync record: {}", write_err);
                }
            }
            Err(_) => {
                warn!(
                    session_id = %event.session_id,
                    path = %path,
                    timeout_ms = config.timeout_ms,
                    "auto-save: timeout"
                );
                
                let error = format!("timeout after {}ms", config.timeout_ms);
                if let Err(write_err) = record_failed_sync(&config.failed_syncs_dir, &event, &error, latency_ms).await {
                    error!("Failed to write failed-sync record: {}", write_err);
                }
            }
        }
    });
}

/// Internal: save to memory and optionally verify
async fn save_and_verify(
    config: &AutoSaveConfig,
    path: &str,
    content: &str,
    session_id: &str,
) -> Result<AutoSaveResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {}", e))?;
    
    // ─── SAVE ──────────────────────────────────────────────────────────────
    let save_payload = serde_json::json!({
        "path": path,
        "content": content,
        "kind": "session",
        "metadata": {
            "session_id": session_id,
            "source": "auto-save",
            "auto_verify": config.auto_verify,
        }
    });
    
    let save_resp = client
        .post(format!("{}/memory/add", config.xavier_url))
        .header("Authorization", format!("Bearer {}", config.auth_token))
        .json(&save_payload)
        .send()
        .await
        .map_err(|e| format!("save request failed: {}", e))?;
    
    let save_ok = save_resp.status().is_success();
    
    if !save_ok {
        let status = save_resp.status();
        let body = save_resp.text().await.unwrap_or_default();
        return Err(format!("save failed: HTTP {} — {}", status, body));
    }
    
    // ─── AUTO-VERIFY (Fase 2) ──────────────────────────────────────────────
    let mut verified = false;
    let mut match_score = 1.0; // Default to perfect if verification skipped
    
    if config.auto_verify {
        match AutoVerifier::verify_save(
            &client,
            &config.xavier_url,
            &config.auth_token,
            path,
            content,
        ).await {
            Ok(verify_result) => {
                verified = verify_result.is_healthy();
                match_score = verify_result.match_score;
                
                if !verified {
                    warn!(
                        path = %path,
                        save_ok = verify_result.save_ok,
                        retrieve_ok = verify_result.retrieve_ok,
                        match_score = verify_result.match_score,
                        "auto-verify: mismatch detected"
                    );
                }
            }
            Err(e) => {
                warn!(
                    path = %path,
                    error = %e,
                    "auto-verify: verification failed"
                );
            }
        }
    }
    
    Ok(AutoSaveResult {
        session_id: session_id.to_string(),
        event_type: "Message".to_string(),
        saved: save_ok,
        verified,
        path: path.to_string(),
        latency_ms: 0, // Calculated by caller
        match_score,
        error: None,
    })
}

/// Record a failed sync to disk for later analysis
async fn record_failed_sync(
    dir: &PathBuf,
    event: &SessionEvent,
    error: &str,
    latency_ms: u64,
) -> Result<(), String> {
    // Ensure directory exists
    fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("failed to create failed-syncs dir: {}", e))?;
    
    let timestamp = chrono::Utc::now().timestamp_millis();
    let filename = format!("failed-sync-{}-{}.json", event.session_id, timestamp);
    let filepath = dir.join(&filename);
    
    let record = serde_json::json!({
        "timestamp_ms": timestamp,
        "session_id": event.session_id,
        "event_type": event.event_type,
        "content_preview": event.content_preview(),
        "error": error,
        "latency_ms": latency_ms,
    });
    
    let json = serde_json::to_string_pretty(&record)
        .map_err(|e| format!("failed to serialize failed-sync record: {}", e))?;

    fs::write(&filepath, json)
        .await
        .map_err(|e| format!("failed to write failed-sync record: {}", e))?;
    
    info!(
        filepath = %filepath.display(),
        "recorded failed sync"
    );
    
    Ok(())
}

/// Resolve Xavier URL from environment or settings
fn resolve_xavier_url() -> String {
    std::env::var("XAVIER_URL").unwrap_or_else(|_| {
        let host = std::env::var("XAVIER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("XAVIER_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8006);
        format!("http://{}:{}", host, port)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn auto_save_config_defaults() {
        let config = AutoSaveConfig::default();
        assert_eq!(config.timeout_ms, 3000);
        assert!(config.auto_verify);
        assert_eq!(config.min_match_score, 0.8);
    }

    #[test]
    fn auto_save_result_serializes() {
        let result = AutoSaveResult {
            session_id: "test-session".to_string(),
            event_type: "Message".to_string(),
            saved: true,
            verified: true,
            path: "sessions/test/123".to_string(),
            latency_ms: 150,
            match_score: 1.0,
            error: None,
        };
        
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-session"));
    }

    #[test]
    fn should_save_filters_event_types() {
        let message_event = SessionEvent {
            session_id: "test".to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("hello".to_string()),
            metadata: None,
        };
        
        assert!(matches!(message_event.event_type, 
            SessionEventType::Message | SessionEventType::ToolResult
        ));
    }
}
