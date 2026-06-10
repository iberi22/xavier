//! # Fixer
//!
//! Generates GitHub Issues and Pull Requests for detected errors.
//! Uses the `gh` CLI or direct GitHub API via `reqwest`.
//!
//! ## Process
//!
//! 1. Analyzer produces diagnosis (root cause + fix suggestion)
//! 2. Fixer creates a formatted GitHub Issue
//! 3. If fix is high-confidence and automatic â†’ GitHub PR
//! 4. Issue/PR is linked back to the service_log entry

use std::process::Command;

use serde::{Deserialize, Serialize};

use super::analyzer::{ErrorDiagnosis, Urgency};
use super::service_log::ServiceLogStore;

/// GitHub repository info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    pub owner: String,
    pub repo: String,
}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            owner: "iberi22".to_string(),
            repo: "xavier".to_string(),
        }
    }
}

/// Result of a fixer action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixerResult {
    pub action: FixerAction,
    pub url: Option<String>,
    pub number: Option<u64>,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FixerAction {
    IssueCreated,
    PullRequestCreated,
    TelegramNotified,
    Skipped,
}

/// The fixer â€” creates GitHub Issues/PRs from error diagnoses.
pub struct Fixer {
    repo: RepoConfig,
    store: Option<ServiceLogStore>,
}

impl Fixer {
    /// Create a new fixer.
    pub fn new() -> Self {
        let store = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { ServiceLogStore::new().await.ok() })
        });

        Self {
            repo: RepoConfig::default(),
            store,
        }
    }

    /// Create with custom repo config.
    pub fn with_repo(repo: RepoConfig) -> Self {
        let store = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { ServiceLogStore::new().await.ok() })
        });

        Self { repo, store }
    }

    /// Process a diagnosis and take action based on urgency.
    pub async fn process_diagnosis(&self, diagnosis: &ErrorDiagnosis) -> FixerResult {
        match diagnosis.urgency {
            Urgency::Critical | Urgency::High => {
                // Create GitHub Issue for critical/high errors
                self.create_issue(diagnosis).await
            }
            Urgency::Medium => {
                // Create issue but lower priority
                let mut result = self.create_issue(diagnosis).await;
                result.message = format!("[Medium Priority] {}", result.message);
                result
            }
            Urgency::Low => {
                // Log to service_log only
                FixerResult {
                    action: FixerAction::Skipped,
                    url: None,
                    number: None,
                    success: true,
                    message: "Low urgency â€” logged for periodic report".to_string(),
                }
            }
        }
    }

    /// Create a GitHub Issue using `gh` CLI.
    async fn create_issue(&self, diagnosis: &ErrorDiagnosis) -> FixerResult {
        let title = format!(
            "[auto] {} error in {}: {}",
            diagnosis.urgency,
            diagnosis.pattern.module,
            &diagnosis.pattern.sample_message[..80.min(diagnosis.pattern.sample_message.len())],
        );

        let body = self.format_issue_body(diagnosis);

        // Try gh CLI first
        match Command::new("gh")
            .args([
                "issue",
                "create",
                "--repo",
                &format!("{}/{}", self.repo.owner, self.repo.repo),
                "--title",
                &title,
                "--body",
                &body,
                "--label",
                &format!("auto-detected,{}", diagnosis.urgency),
            ])
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if output.status.success() {
                    let url = stdout.trim().to_string();
                    let number = url.rsplit('/').next().and_then(|s| s.parse::<u64>().ok());

                    // Update the log entries in DB
                    if let Some(ref store) = self.store {
                        let resolution = serde_json::json!({
                            "action": "issue_created",
                            "url": url,
                            "issue_number": number,
                            "diagnosis": {
                                "root_cause": diagnosis.root_cause,
                                "suggested_fix": diagnosis.suggested_fix,
                                "confidence": diagnosis.confidence,
                            }
                        });

                        let _ = store.resolve(&diagnosis.pattern.module, resolution).await;
                    }

                    FixerResult {
                        action: FixerAction::IssueCreated,
                        url: Some(url),
                        number,
                        success: true,
                        message: format!("Issue created: {}", title),
                    }
                } else {
                    FixerResult {
                        action: FixerAction::IssueCreated,
                        url: None,
                        number: None,
                        success: false,
                        message: format!("Failed to create issue: {}", stderr.trim()),
                    }
                }
            }
            Err(e) => FixerResult {
                action: FixerAction::IssueCreated,
                url: None,
                number: None,
                success: false,
                message: format!("gh CLI not available: {}", e),
            },
        }
    }

    /// Format a GitHub issue body from the diagnosis.
    fn format_issue_body(&self, diagnosis: &ErrorDiagnosis) -> String {
        let confidence_pct = diagnosis.confidence * 100.0;
        format!(
            r#"## ðŸ¤– Auto-Detected Error

**Module:** `{}`
**Level:** {}
**Frequency:** {} times
**First seen:** {}
**Last seen:** {}
**Urgency:** {}

## Analysis

**Root Cause:**
{}

**Suggested Fix:**
{}

## Source Location
{}

---
*Auto-generated by Xavier Observability | Confidence: {:.0}%*
"#,
            diagnosis.pattern.module,
            diagnosis.pattern.level,
            diagnosis.pattern.frequency,
            diagnosis.pattern.first_seen,
            diagnosis.pattern.last_seen,
            diagnosis.urgency,
            diagnosis.root_cause,
            diagnosis.suggested_fix,
            diagnosis.source_location.as_deref().unwrap_or("(unknown)"),
            confidence_pct,
        )
    }
}

impl Default for Fixer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::analyzer::Urgency;
    use crate::observability::service_log::{ErrorPattern, LogLevel};

    fn make_diagnosis(urgency: Urgency) -> ErrorDiagnosis {
        ErrorDiagnosis {
            pattern: ErrorPattern {
                module: "test::mod".into(),
                level: LogLevel::Error,
                frequency: 5,
                sample_message: "connection timeout".into(),
                first_seen: "2025-01-01T00:00:00Z".into(),
                last_seen: "2025-01-01T01:00:00Z".into(),
            },
            analyzed_at: "2025-01-01T02:00:00.000Z".into(),
            root_cause: "Network timeout".into(),
            source_location: Some("src/server/http.rs:42".into()),
            suggested_fix: "Increase timeout".into(),
            confidence: 0.85,
            urgency,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_format_issue_body() {
        // Fixer::new() uses tokio block_in_place, so we need a tokio runtime
        let fixer = Fixer::new();
        let diagnosis = make_diagnosis(Urgency::High);
        let body = fixer.format_issue_body(&diagnosis);
        assert!(body.contains("test::mod"));
        assert!(body.contains("Network timeout"));
        assert!(body.contains("Increase timeout"));
        assert!(body.contains("src/server/http.rs:42"));
        assert!(body.contains("85%"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_format_issue_body_without_location() {
        let fixer = Fixer::new();
        let mut diagnosis = make_diagnosis(Urgency::Critical);
        diagnosis.source_location = None;
        let body = fixer.format_issue_body(&diagnosis);
        assert!(body.contains("(unknown)"));
    }

    #[test]
    fn test_fixer_action_display() {
        let actions = [
            FixerAction::IssueCreated,
            FixerAction::PullRequestCreated,
            FixerAction::TelegramNotified,
            FixerAction::Skipped,
        ];
        // Just verify they debug-format without panic
        for action in &actions {
            let _ = format!("{:?}", action);
        }
    }

    #[test]
    fn test_fixer_result_struct() {
        let result = FixerResult {
            action: FixerAction::IssueCreated,
            url: Some("https://github.com/iberi22/xavier/issues/1".into()),
            number: Some(1),
            success: true,
            message: "Issue created".into(),
        };
        assert!(result.success);
        assert_eq!(result.number, Some(1));
        assert_eq!(
            result.url.as_deref(),
            Some("https://github.com/iberi22/xavier/issues/1")
        );
    }

    #[test]
    fn test_repo_config_defaults() {
        let config = RepoConfig::default();
        assert_eq!(config.owner, "iberi22");
        assert_eq!(config.repo, "xavier");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_fixer_format_body_has_module() {
        let fixer = Fixer::new();
        let diagnosis = make_diagnosis(Urgency::Low);
        let body = fixer.format_issue_body(&diagnosis);
        assert!(body.contains("test::mod"));
        assert!(body.contains("Network timeout"));
        assert!(body.contains("85%"));
    }

    #[test]
    fn test_issue_title_truncation() {
        let diagnosis = make_diagnosis(Urgency::High);
        let sample = &diagnosis.pattern.sample_message;
        let truncated = &sample[..80.min(sample.len())];
        assert_eq!(truncated, "connection timeout");
    }
}
