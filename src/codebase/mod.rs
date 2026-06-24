//! Codebase module — per-project SQLite databases.
//!
//! Each repository gets its own `.xavier/codebase.db` (git + code data)
//! plus a separate private conversations DB at `~/.xavier/conversations/{project_id}.db`.

use anyhow::{anyhow, Result};
use regex::Regex;
use std::sync::LazyLock;

pub mod connection_manager;
pub mod conversations_db;
pub mod db;
pub mod maturity;

/// Validation regex for project_id: only alphanumeric, hyphens, and underscores.
static PROJECT_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_-]+$").expect("invalid project_id regex"));

/// Validates that a project_id is safe to use in file paths.
///
/// It must be non-empty and contain only alphanumeric characters, hyphens, or underscores.
/// This prevents path traversal attacks by rejecting '/', '..', '\', '~', etc.
pub fn validate_project_id(project_id: &str) -> Result<()> {
    if project_id.is_empty() {
        return Err(anyhow!("project_id cannot be empty"));
    }
    if !PROJECT_ID_RE.is_match(project_id) {
        return Err(anyhow!(
            "Invalid project_id: '{}'. Only alphanumeric characters, hyphens, and underscores are allowed.",
            project_id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_project_id() {
        assert!(validate_project_id("my-project").is_ok());
        assert!(validate_project_id("project_123").is_ok());
        assert!(validate_project_id("valid_ID-45").is_ok());

        assert!(validate_project_id("").is_err());
        assert!(validate_project_id("../etc/passwd").is_err());
        assert!(validate_project_id("my/project").is_err());
        assert!(validate_project_id("project\\name").is_err());
        assert!(validate_project_id("~").is_err());
        assert!(validate_project_id("project name").is_err());
        assert!(validate_project_id("project@123").is_err());
    }
}
