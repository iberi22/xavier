use crate::security::audit::AuditLogger;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Audit entry for classified content read requests.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ClearanceReadAudit {
    pub subject: String,
    pub role: String,
    pub action: String,
    pub route: String,
    pub allowed: bool,
    pub reason: Option<String>,
    pub query_hash: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl ClearanceReadAudit {
    /// Creates a new clearance read audit record timestamped with the current UTC time.
    pub fn new(
        subject: impl Into<String>,
        role: impl Into<String>,
        action: impl Into<String>,
        route: impl Into<String>,
        allowed: bool,
        reason: Option<String>,
        query_hash: Option<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            role: role.into(),
            action: action.into(),
            route: route.into(),
            allowed,
            reason,
            query_hash,
            timestamp: Utc::now(),
        }
    }

    /// Returns the subject and role as a tuple.
    pub fn subject_and_role(&self) -> (&str, &str) {
        (&self.subject, &self.role)
    }
}

/// Helper to compute SHA-256 hex digest of a search query or payload.
pub fn query_hash(query: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(query.as_bytes());
    crate::utils::crypto::hex_encode(&hasher.finalize())
}

/// Helper that returns subject and role as a tuple of owned strings.
pub fn subject_and_role(subject: &str, role: &str) -> (String, String) {
    (subject.to_string(), role.to_string())
}

/// Resolves target JSONL audit file path from env `XAVIER_CLEARANCE_AUDIT_PATH` or default.
pub fn get_audit_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("XAVIER_CLEARANCE_AUDIT_PATH") {
        PathBuf::from(path)
    } else {
        PathBuf::from("data/security/clearance_audit.jsonl")
    }
}

/// Appends a single audit entry to the designated JSONL file path.
pub fn append_at(path: &Path, entry: &ClearanceReadAudit) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    let json = serde_json::to_string(entry)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Appends a clearance audit entry to the default JSONL audit sink.
pub fn record(entry: ClearanceReadAudit) -> Result<()> {
    let path = get_audit_file_path();
    if let Err(e) = append_at(&path, &entry) {
        tracing::error!("Failed to record clearance audit JSONL entry: {}", e);
    }
    Ok(())
}

/// Records a denied clearance audit entry.
pub fn record_denied(
    subject: impl Into<String>,
    role: impl Into<String>,
    action: impl Into<String>,
    route: impl Into<String>,
    reason: impl Into<String>,
) -> Result<()> {
    let entry = ClearanceReadAudit::new(
        subject,
        role,
        action,
        route,
        false,
        Some(reason.into()),
        None,
    );
    record(entry)
}

/// Maps a clearance read decision into the canonical SQLite audit log via AuditLogger.
///
/// Permission string format: `clearance:<action>:<route>`
pub async fn mirror_to_audit_logger(entry: &ClearanceReadAudit) -> Result<()> {
    let logger = AuditLogger::new();
    let permission = format!("clearance:{}:{}", entry.action, entry.route);
    if let Err(e) = logger
        .log_check(&entry.subject, &entry.role, &permission, entry.allowed)
        .await
    {
        tracing::error!("Failed to mirror clearance audit check to SQLite: {}", e);
    }
    Ok(())
}

/// Appends to the JSONL sink and mirrors to SQLite if `XAVIER_CLEARANCE_AUDIT_SQLITE` != "0".
/// Fail-open: errors on mirroring or JSONL writing do not bubble up or interrupt execution.
pub async fn record_and_mirror(entry: ClearanceReadAudit) -> Result<()> {
    if let Err(e) = record(entry.clone()) {
        tracing::error!(
            "Failed to write clearance audit entry in record_and_mirror: {}",
            e
        );
    }

    let sqlite_enabled =
        std::env::var("XAVIER_CLEARANCE_AUDIT_SQLITE").unwrap_or_else(|_| "1".to_string()) != "0";

    if sqlite_enabled {
        if let Err(e) = mirror_to_audit_logger(&entry).await {
            tracing::error!("Failed to mirror clearance audit to SQLite: {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_clearance_read_audit_creation_and_helpers() {
        let entry = ClearanceReadAudit::new(
            "user-1",
            "admin",
            "search",
            "/v1/memory/search",
            true,
            None,
            Some(query_hash("test query")),
        );

        assert_eq!(entry.subject, "user-1");
        assert_eq!(entry.role, "admin");
        assert_eq!(entry.action, "search");
        assert_eq!(entry.route, "/v1/memory/search");
        assert!(entry.allowed);
        assert_eq!(entry.subject_and_role(), ("user-1", "admin"));
        assert_eq!(
            subject_and_role("user-1", "admin"),
            ("user-1".to_string(), "admin".to_string())
        );
        assert_eq!(
            query_hash("test query"),
            "050579eeae87a0436e1ff56d7a8388c2ee9b71b5f9170eb7aaaec1bcb405ca12"
        );
    }

    #[test]
    fn test_append_at_jsonl() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let entry = ClearanceReadAudit::new(
            "user-2",
            "analyst",
            "read",
            "/v1/documents/doc-123",
            true,
            None,
            None,
        );

        append_at(path, &entry).unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"subject\":\"user-2\""));
        assert!(content.contains("\"action\":\"read\""));
        assert!(content.contains("\"allowed\":true"));
    }

    #[test]
    fn test_record_denied_creates_denied_entry() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();
        std::env::set_var("XAVIER_CLEARANCE_AUDIT_PATH", path);

        record_denied(
            "user-3",
            "readonly",
            "export",
            "/v1/memory/export",
            "insufficient clearance",
        )
        .unwrap();

        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("\"subject\":\"user-3\""));
        assert!(content.contains("\"allowed\":false"));
        assert!(content.contains("\"reason\":\"insufficient clearance\""));

        std::env::remove_var("XAVIER_CLEARANCE_AUDIT_PATH");
    }

    #[tokio::test]
    async fn test_mirror_to_audit_logger_fail_open() {
        let entry = ClearanceReadAudit::new(
            "user-4",
            "guest",
            "read",
            "/v1/classified/top_secret",
            false,
            Some("denied".into()),
            None,
        );

        // ConnectionManager global is uninitialized here, mirror_to_audit_logger should log and return Ok(())
        let result = mirror_to_audit_logger(&entry).await;
        assert!(result.is_ok());
    }
}
