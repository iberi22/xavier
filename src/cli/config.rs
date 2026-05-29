//! CLI configuration utilities.
//!
//! # Canonical Client Configuration Contract
//!
//! The CLI follows this priority order when resolving the Xavier server URL:
//!
//! 1. **`XAVIER_URL`** (env var) — Full base URL, e.g. `http://xavier:8006`
//! 2. **`XAVIER_HOST` + `XAVIER_PORT`** (env vars) — Component parts assembled into a URL
//! 3. **Settings file / defaults** — From `XavierSettings::current()` (default `http://127.0.0.1:8006`)
//!
//! `XAVIER_URL` is the canonical configuration variable. All CLI commands (`add`,
//! `search`, `stats`, etc.) resolve the server URL through this module.

use anyhow::Result;
use std::path::PathBuf;

use crate::settings::XavierSettings;

pub fn resolve_http_token() -> Result<String> {
    xavier::security::auth::resolve_xavier_token()
}


pub fn resolve_http_bind_host() -> String {
    std::env::var("XAVIER_HOST").unwrap_or_else(|_| XavierSettings::current().server.host)
}

/// Resolve a base URL for a given port.
///
/// Priority:
/// 1. `XAVIER_URL` env var (canonical)
/// 2. Construct from settings host + given port
/// 3. Settings default `client_base_url()` if port matches default
pub fn resolve_base_url_for_port(port: u16) -> String {
    std::env::var("XAVIER_URL").unwrap_or_else(|_| {
        let settings = XavierSettings::current();
        if port == settings.server.port {
            return settings.client_base_url();
        }
        let host = resolve_http_bind_host();
        format!("http://{}:{}", host, port)
    })
}

/// Resolve the Xavier server base URL using the canonical contract.
///
/// Priority:
/// 1. `XAVIER_URL` env var
/// 2. `XAVIER_HOST` + `XAVIER_PORT` env vars assembled into a URL
/// 3. Settings file / defaults (`http://127.0.0.1:8006`)
pub fn resolve_base_url() -> String {
    let port = resolve_http_port();
    resolve_base_url_for_port(port)
}

/// Resolve the HTTP port using the canonical contract.
///
/// Priority:
/// 1. `XAVIER_PORT` env var
/// 2. Settings file / defaults (8006)
pub fn resolve_http_port() -> u16 {
    std::env::var("XAVIER_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| XavierSettings::current().server.port)
}

pub fn xavier_token() -> String {
    xavier::security::auth::resolve_xavier_token().expect("XAVIER_TOKEN environment variable must be set for CLI client commands")
}

pub fn require_xavier_token() -> Result<String> {
    xavier::security::auth::resolve_xavier_token()
}

pub fn code_graph_db_path() -> PathBuf {
    std::env::var("XAVIER_CODE_GRAPH_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| XavierSettings::resolve_data_dir().join("code_graph.db"))
}

pub fn state_panel_root(workspace_dir: &std::path::Path, workspace_id: &str) -> PathBuf {
    std::env::var("XAVIER_PANEL_STORE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            workspace_dir
                .join("data")
                .join("workspaces")
                .join(workspace_id)
                .join("panel_threads")
        })
}

