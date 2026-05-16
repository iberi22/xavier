//! CLI configuration utilities

use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::settings::XavierSettings;

pub fn resolve_http_token() -> Result<String> {
    XavierSettings::current().auth_token.ok_or_else(|| {
        anyhow!("XAVIER_TOKEN environment variable must be set to start the HTTP server.")
    })
}


pub fn resolve_http_bind_host() -> String {
    std::env::var("XAVIER_HOST").unwrap_or_else(|_| XavierSettings::current().server.host)
}

pub fn resolve_base_url_for_port(port: u16) -> String {
    std::env::var("XAVIER_URL").unwrap_or_else(|_| {
        let settings = XavierSettings::current();
        if port == settings.server.port {
            return settings.client_base_url();
        }
        let host = match settings.server.host.as_str() {
            "0.0.0.0" | "::" => "127.0.0.1",
            other => other,
        };
        format!("http://{}:{}", host, port)
    })
}

pub fn resolve_base_url() -> String {
    let port = resolve_http_port();
    resolve_base_url_for_port(port)
}

pub fn resolve_http_port() -> u16 {
    std::env::var("XAVIER_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_else(|| XavierSettings::current().server.port)
}

pub fn xavier_token() -> String {
    XavierSettings::current()
        .auth_token
        .expect("XAVIER_TOKEN environment variable must be set for CLI client commands")
}

pub fn require_xavier_token() -> Result<String> {
    XavierSettings::current()
        .auth_token
        .ok_or_else(|| anyhow!("XAVIER_TOKEN environment variable must be set for CLI client commands"))
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

