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
//!
//! # HTTP client honesty
//!
//! Callers must distinguish [`CliHttpOutcome`] kinds: auth failures (401/403) are
//! never reported as "server offline" unless `--offline-ok` is explicit.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::settings::XavierSettings;

/// Classified outcome of a CLI → Xavier HTTP call (before parsing the body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliHttpOutcome {
    /// Token rejected by the server (HTTP 401 or 403).
    AuthFailed { status: u16 },
    /// TCP/connect failure (server down, refused, DNS, etc.).
    ConnectionRefused { detail: String },
    /// HTTP response that is neither success nor auth failure.
    HttpError { status: u16, body: String },
}

/// Actionable message printed on AUTH_FAILED (and used in `anyhow` errors).
pub fn auth_failed_message(status: u16) -> String {
    format!(
        "AUTH_FAILED (HTTP {status}): Xavier token rejected. \
Restart `xavier http` after sourcing `.env`, or align the systemd EnvironmentFile \
with the CLI token so both use the same XAVIER_TOKEN."
    )
}

/// True when the status is 401 Unauthorized or 403 Forbidden.
pub fn is_auth_failure(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 401 || status.as_u16() == 403
}

/// Classify a successful `reqwest` response that is not `2xx`.
pub fn classify_error_response(status: reqwest::StatusCode, body: String) -> CliHttpOutcome {
    if is_auth_failure(status) {
        CliHttpOutcome::AuthFailed {
            status: status.as_u16(),
        }
    } else {
        CliHttpOutcome::HttpError {
            status: status.as_u16(),
            body,
        }
    }
}

/// Classify a `reqwest` transport error (no HTTP status).
pub fn classify_transport_error(err: &reqwest::Error) -> CliHttpOutcome {
    let detail = err.to_string();
    if err.is_connect()
        || err.is_timeout()
        || detail.to_ascii_lowercase().contains("connection refused")
        || detail.to_ascii_lowercase().contains("dns error")
        || detail
            .to_ascii_lowercase()
            .contains("name or service not known")
    {
        CliHttpOutcome::ConnectionRefused { detail }
    } else {
        // Treat ambiguous transport failures as connection problems for offline fallback.
        CliHttpOutcome::ConnectionRefused { detail }
    }
}

/// Fail closed on auth unless `offline_ok` allows an explicit offline path.
pub fn auth_failed_error(status: u16) -> anyhow::Error {
    anyhow!("{}", auth_failed_message(status))
}

/// Reject Windows-style absolute paths for `XAVIER_DATA_DIR` on Unix hosts.
pub fn validate_data_dir_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        let bytes = trimmed.as_bytes();
        // Drive-letter paths: `E:\...`, `E:/...`, `C:...`
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return Err(format!(
                "XAVIER_DATA_DIR looks like a Windows path ('{trimmed}'). \
On Linux/Unix use a POSIX path, e.g. /home/you/proyectosSWAL/xavier/data"
            ));
        }
        // UNC / extended Windows prefixes
        if trimmed.starts_with("\\\\")
            || trimmed.starts_with("//?/")
            || trimmed.starts_with("\\\\?\\")
        {
            return Err(format!(
                "XAVIER_DATA_DIR looks like a Windows UNC path ('{trimmed}'). \
On Linux/Unix use a POSIX path under /home or the repo `data/` directory"
            ));
        }
    }

    let _ = trimmed;
    Ok(())
}

/// Validate `XAVIER_DATA_DIR` from the environment when set.
pub fn validate_xavier_data_dir_env() -> Result<()> {
    if let Ok(path) = std::env::var("XAVIER_DATA_DIR") {
        if let Err(msg) = validate_data_dir_path(&path) {
            return Err(anyhow!("{msg}"));
        }
    }
    Ok(())
}

/// Resolve http token.
pub fn resolve_http_token() -> Result<String> {
    Ok(xavier::security::auth::resolve_xavier_token())
}

/// Resolve http bind host.
pub fn resolve_http_bind_host() -> String {
    // If we are in headless mode or the user hasn't specified a host,
    // we default to 127.0.0.1 for security.
    std::env::var("XAVIER_HOST").unwrap_or_else(|_| {
        let settings = XavierSettings::current();
        // Check for specific headless marker or default to 127.0.0.1
        if std::env::var("XAVIER_HEADLESS").is_ok() {
            "127.0.0.1".to_string()
        } else {
            settings.server.host.clone()
        }
    })
}

/// Resolve a base URL for a given port.
///
/// Priority:
/// 1. `XAVIER_URL` env var (canonical)
/// 2. Construct from settings host + given port
/// 3. Settings default `client_base_url()` if port matches default
pub fn resolve_base_url_for_port(port: u16) -> String {
    std::env::var("XAVIER_URL").unwrap_or_else(|_| {
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

/// Resolve the MCP HTTP+SSE server port.
///
/// Priority:
/// 1. Explicit `--mcp-port` flag value
/// 2. `XAVIER_MCP_PORT` env var
/// 3. Default `8100`
///
/// A resolved value of `0` disables the MCP HTTP server (useful when running
/// `xavier http` without the MCP endpoint, or to avoid a port conflict).
pub fn resolve_mcp_port(flag: Option<u16>) -> u16 {
    flag.or_else(|| {
        std::env::var("XAVIER_MCP_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
    })
    .unwrap_or(crate::cli::mcp::DEFAULT_MCP_PORT)
}

/// Xavier token.
pub fn xavier_token() -> String {
    xavier::security::auth::resolve_xavier_token()
}

/// Require xavier token.
pub fn require_xavier_token() -> Result<String> {
    Ok(xavier::security::auth::resolve_xavier_token())
}

/// Code graph db path (canonical — same as HTTP server / doctor / sync).
pub fn code_graph_db_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    xavier::codebase::codegraph_paths::code_graph_db_path_for(&cwd)
}

/// State panel root.
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

/// Resolve cwd.
pub fn resolve_cwd() -> String {
    let cwd_file = XavierSettings::resolve_data_dir().join("cwd");
    if let Ok(cwd) = std::fs::read_to_string(&cwd_file) {
        cwd.trim().to_string()
    } else {
        "/".to_string()
    }
}

/// Save cwd.
pub fn save_cwd(path: &str) -> Result<()> {
    let cwd_file = XavierSettings::resolve_data_dir().join("cwd");
    if let Some(parent) = cwd_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cwd_file, path)?;
    Ok(())
}

#[cfg(test)]
mod data_dir_tests {
    use super::*;

    #[test]
    fn rejects_windows_drive_letter_on_unix() {
        #[cfg(unix)]
        {
            assert!(validate_data_dir_path(r"E:\scripts-python\xavier\data").is_err());
            assert!(validate_data_dir_path("C:/Users/belal/xavier/data").is_err());
        }
    }

    #[test]
    fn accepts_posix_paths() {
        assert!(validate_data_dir_path("/home/belal/proyectosSWAL/xavier/data").is_ok());
        assert!(validate_data_dir_path("data").is_ok());
        assert!(validate_data_dir_path("").is_ok());
    }
}
