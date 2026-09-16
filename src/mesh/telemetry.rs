use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Represents an anonymized telemetry payload (Data Commons Phase 2).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TelemetryPayload {
    pub node_type: String,
    pub event_kind: String,
    pub sanitized_message: String,
    pub error_stack: Option<String>,
    pub timestamp: u64,
    pub classification: String,
}

static PII_REGEXES: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        // Emails
        Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
        // IPv4
        Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b").unwrap(),
        // Windows Paths (e.g., C:\Users\name\...)
        Regex::new(r"[a-zA-Z]:\\[\\\S|*\S]*").unwrap(),
        // Unix absolute paths (e.g., /home/name/...)
        Regex::new(r"/(?:home|Users)/[a-zA-Z0-9_-]+/[/\S]*").unwrap(),
    ]
});

/// Scrubs PII (Emails, IPs, Local Paths) from a given string.
pub fn scrub_pii(input: &str) -> String {
    let mut scrubbed = input.to_string();
    for re in PII_REGEXES.iter() {
        scrubbed = re.replace_all(&scrubbed, "[REDACTED]").to_string();
    }
    scrubbed
}

impl TelemetryPayload {
    /// Creates a new payload, scrubbing all fields automatically.
    pub fn new_scrubbed(event_kind: &str, raw_message: &str, raw_stack: Option<&str>) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            node_type: "xavier_client".to_string(),
            event_kind: event_kind.to_string(),
            sanitized_message: scrub_pii(raw_message),
            error_stack: raw_stack.map(scrub_pii),
            timestamp,
            classification: "INTERNAL".to_string(),
        }
    }

    /// Creates a new payload tagged as INTERNAL with PII scrubbed.
    pub fn new_internal(event_kind: &str, raw_message: &str, raw_stack: Option<&str>) -> Self {
        Self::new_scrubbed(event_kind, raw_message, raw_stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_pii() {
        let input = "Error at C:\\Users\\belal\\scripts-python\\xavier\\src\\main.rs. Contact admin@domain.com from IP 192.168.1.5";
        // We use a simpler check since regex might match spaces differently depending on exact bounds
        let output = scrub_pii(input);
        assert!(!output.contains("belal"));
        assert!(!output.contains("admin@domain.com"));
        assert!(!output.contains("192.168.1.5"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn test_telemetry_payload() {
        let payload = TelemetryPayload::new_scrubbed("panic", "Crash in /home/user/project", None);
        assert_eq!(payload.sanitized_message, "Crash in [REDACTED]");
    }

    #[test]
    fn test_new_internal_classification_default() {
        let payload = TelemetryPayload::new_internal("info", "System startup complete", None);
        assert_eq!(payload.classification, "INTERNAL");
        assert_eq!(payload.node_type, "xavier_client");
        assert_eq!(payload.event_kind, "info");
    }

    #[test]
    fn test_new_internal_pii_scrubbed() {
        let payload = TelemetryPayload::new_internal(
            "error",
            "Failed connection to user@example.com at 10.0.0.1",
            Some("Traceback at /home/user/app.rs"),
        );
        assert_eq!(payload.classification, "INTERNAL");
        assert!(!payload.sanitized_message.contains("user@example.com"));
        assert!(!payload.sanitized_message.contains("10.0.0.1"));
        assert!(payload.sanitized_message.contains("[REDACTED]"));
        assert!(payload.error_stack.is_some());
        assert!(!payload
            .error_stack
            .as_ref()
            .unwrap()
            .contains("/home/user/app.rs"));
        assert!(payload.error_stack.as_ref().unwrap().contains("[REDACTED]"));
    }
}
