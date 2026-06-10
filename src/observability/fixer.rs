//! # Fixer
//!
//! Generates GitHub Issues and Pull Requests for detected errors.
//! Uses the `gh` CLI or direct GitHub API via `reqwest`.
//!
//! ## Process
//!
//! 1. Analyzer produces diagnosis (root cause + fix suggestion)
//! 2. Fixer creates a formatted GitHub Issue
//! 3. If fix is high-confidence and automatic → GitHub PR
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

/// The fixer — creates GitHub Issues/PRs from error diagnoses.
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
                    message: "Low urgency — logged for periodic report".to_string(),
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
            r#"## 🤖 Auto-Detected Error

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
