//! # Reporter — Maturity Report Generation
//!
//! Generates the feature-maturity.json report and markdown summaries.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::maturity::scorer::ScoredFeature;
use crate::maturity::MaturityResult;

/// Summary of the full maturity scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub overall_maturity: u8,
    pub total_features: usize,
    pub production_ready: usize,
    pub needs_work: usize,
    pub in_progress: usize,
    pub scan_errors: usize,
}

/// Full report matching the feature-maturity.json format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaturityReport {
    pub schema: String,
    pub meta: ReportMeta,
    pub summary: Summary,
    pub features: Vec<ReportFeature>,
    pub sprint: SprintInfo,
    /// History of past scans for trend tracking (v2)
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMeta {
    pub format_version: String,
    pub generated: String,
    pub head: String,
    pub scanner: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportFeature {
    pub id: String,
    pub name: String,
    pub priority: String,
    pub maturity_percent: u8,
    pub status: String,
    pub subcomponents: Vec<ReportSubcomponent>,
    pub loc_estimate: Option<usize>,
    pub whats_missing: Vec<String>,
    /// Memory evidence score (v2 deep-scan)
    #[serde(default)]
    pub memory_usage: u8,
    /// Issue health score (v2 deep-scan)
    #[serde(default)]
    pub issue_health: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSubcomponent {
    pub name: String,
    pub maturity: u8,
    pub weight: u32,
    pub status: String,
    pub tests_passing: usize,
    pub tests_total: usize,
    pub symbols_found: u8,
    pub symbols_total: u8,
    /// Memory evidence for this subcomponent (v2 deep-scan)
    #[serde(default)]
    pub memory_usage: u8,
    /// Issue health for this subcomponent (v2 deep-scan)
    #[serde(default)]
    pub issue_health: u8,
    /// Evidence detail string
    #[serde(default)]
    pub evidence_detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprintInfo {
    pub active: bool,
    pub id: String,
    pub target: u8,
    pub active_issues: Vec<IssueRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueRef {
    pub number: u32,
    pub title: String,
}

/// One entry in the history array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub generated: String,
    pub head: String,
    pub overall_maturity: u8,
    pub scanner: String,
}

impl MaturityReport {
    /// Build a report from a MaturityResult.
    pub fn from_result(
        result: MaturityResult,
        sprint_id: &str,
        sprint_target: u8,
        active_issues: Vec<IssueRef>,
        previous_history: Vec<HistoryEntry>,
    ) -> Self {
        Self::from_scored(
            result.features,
            result.summary,
            result.scanned_at,
            result.head_commit,
            sprint_id,
            sprint_target,
            active_issues,
            previous_history,
            result.mcp_enabled,
        )
    }

    /// Build a report from scanned and scored features.
    pub fn from_scored(
        features: Vec<ScoredFeature>,
        summary: Summary,
        scanned_at: String,
        head_commit: String,
        sprint_id: &str,
        sprint_target: u8,
        active_issues: Vec<IssueRef>,
        previous_history: Vec<HistoryEntry>,
        mcp_enabled: bool,
    ) -> Self {
        let report_features: Vec<ReportFeature> = features
            .into_iter()
            .map(|f| {
                let subcomponents: Vec<ReportSubcomponent> = f
                    .subcomponents
                    .into_iter()
                    .map(|s| {
                        let status = if s.maturity >= 90 {
                            "done".to_string()
                        } else if s.tests_total > 0 && s.tests_passing > 0 {
                            format!("jules ({}/{})", s.tests_passing, s.tests_total)
                        } else {
                            "needs_work".to_string()
                        };
                        ReportSubcomponent {
                            name: s.name,
                            maturity: s.maturity,
                            weight: s.weight,
                            status,
                            tests_passing: s.tests_passing,
                            tests_total: s.tests_total,
                            symbols_found: s.symbols_found,
                            symbols_total: s.symbols_total,
                            memory_usage: s.memory_usage,
                            issue_health: s.issue_health,
                            evidence_detail: s.evidence_detail,
                        }
                    })
                    .collect();

                let missing: Vec<String> = subcomponents
                    .iter()
                    .filter(|s| s.maturity < 90)
                    .map(|s| format!("{}: {}% (tests {}/{})", s.name, s.maturity, s.tests_passing, s.tests_total))
                    .collect();

                // Determine memory/issue health as aggregate of subcomponents
                let avg_memory = if subcomponents.is_empty() {
                    0u8
                } else {
                    (subcomponents.iter().map(|s| s.memory_usage as u32).sum::<u32>() / subcomponents.len() as u32) as u8
                };
                let avg_issues = if subcomponents.is_empty() {
                    0u8
                } else {
                    (subcomponents.iter().map(|s| s.issue_health as u32).sum::<u32>() / subcomponents.len() as u32) as u8
                };

                ReportFeature {
                    id: f.id.clone(),
                    name: f.name.clone(),
                    priority: "medium".to_string(),
                    maturity_percent: f.overall as u8,
                    status: f.status.clone(),
                    subcomponents,
                    loc_estimate: None,
                    whats_missing: missing,
                    memory_usage: avg_memory,
                    issue_health: avg_issues,
                }
            })
            .collect();

        // Add current scan to history
        let mut history = previous_history;
        history.push(HistoryEntry {
            generated: scanned_at.clone(),
            head: head_commit.clone(),
            overall_maturity: summary.overall_maturity,
            scanner: "xavier-maturity-scanner-v2".to_string(),
        });
        // Keep last 20 entries
        if history.len() > 20 {
            history = history.split_off(history.len() - 20);
        }

        let scanner_name = if mcp_enabled {
            "xavier-maturity-engine-v2 (MCP-First)"
        } else {
            "xavier-maturity-engine-v2 (Fallback-Grep)"
        };

        Self {
            schema: "xavier.maturity.v2".to_string(),
            meta: ReportMeta {
                format_version: "2.0.0".to_string(),
                generated: scanned_at,
                head: head_commit,
                scanner: scanner_name.to_string(),
            },
            summary,
            features: report_features,
            sprint: SprintInfo {
                active: true,
                id: sprint_id.to_string(),
                target: sprint_target,
                active_issues,
            },
            history,
        }
    }

    /// Write the report to a JSON file.
    pub fn write_json(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialization error: {}", e))?;
        std::fs::write(path, &json)
            .map_err(|e| format!("Write error: {}", e))?;
        Ok(())
    }

    /// Generate a human-readable markdown summary.
    pub fn to_markdown(&self) -> String {
        let mut md = format!(
            "# Maturity Report\n\n**Generated:** `{}`\n**Sprint:** `{}` (target: {}%)\n**Overall:** **{}%** | Production Ready: {}/{} | Needs Work: {} | In Progress: {}\n\n| Feature | % | Status | Tests | Memory | Issues |\n|---------|---|---|-------|-------|\n",
            self.meta.generated,
            self.sprint.id,
            self.sprint.target,
            self.summary.overall_maturity,
            self.summary.production_ready,
            self.summary.total_features,
            self.summary.needs_work,
            self.summary.in_progress,
        );

        for feat in &self.features {
            let status_icon = match feat.status.as_str() {
                "production_ready" => "✅",
                "needs_work" => "⚠️",
                _ => "🔧",
            };
            md.push_str(&format!(
                "| {} {} | {}% | {} | {} | {}% | {}% |\n",
                status_icon, feat.name, feat.maturity_percent, feat.status,
                feat.subcomponents.iter()
                    .map(|s| format!("{}: {}%", s.name, s.maturity))
                    .collect::<Vec<_>>()
                    .join(", "),
                feat.memory_usage, feat.issue_health
            ));
        }

        if !self.history.is_empty() {
            md.push_str("\n## History\n\n");
            md.push_str("| Date | Overall |\n|---|---|\n");
            for entry in self.history.iter().rev().take(10) {
                let date = &entry.generated[..10];
                md.push_str(&format!("| {} | {}% |\n", date, entry.overall_maturity));
            }
        }

        md
    }
}
