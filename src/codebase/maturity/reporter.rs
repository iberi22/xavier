//! # Reporter — Maturity Report Generation (New MaturityEngine)
//!
//! Generates the feature-maturity.json report and human-readable summaries.

use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::Result;
use crate::maturity::scorer::ScoredFeature;

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
    pub scanner: String,
    pub schema_version: String,
    pub timestamp: String,
    pub overall_pct: f64,
    pub summary: Summary,
    pub features: Vec<ScoredFeature>,
}

/// Generate a maturity report from scored features.
pub fn generate_report(scans: Vec<ScoredFeature>, overall: f64) -> MaturityReport {
    let production_ready = scans.iter().filter(|f| f.status == "production_ready").count();
    let needs_work = scans.iter().filter(|f| f.status == "needs_work").count();
    let in_progress = scans.iter().filter(|f| f.status == "in_progress").count();

    let summary = Summary {
        overall_maturity: overall as u8,
        total_features: scans.len(),
        production_ready,
        needs_work,
        in_progress,
        scan_errors: 0, // MaturityEngine handles errors before this
    };

    MaturityReport {
        scanner: "xavier-maturity-engine-v3".to_string(),
        schema_version: "3.0.0".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        overall_pct: overall,
        summary,
        features: scans,
    }
}

/// Save the maturity report to a JSON file.
pub fn save_report(path: &Path, report: &MaturityReport) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)?;
    Ok(())
}

/// Format the maturity report for CLI output.
pub fn format_report(report: &MaturityReport) -> String {
    let mut output = String::new();
    output.push_str("\n Xavier Maturity Report (v3)\n");
    output.push_str(&format!(" {}\n", "=".repeat(40)));
    output.push_str(&format!(" Overall Maturity: {:.1}%\n", report.overall_pct));
    output.push_str(&format!(" Features: {} ({} ready, {} work, {} progress)\n",
        report.summary.total_features,
        report.summary.production_ready,
        report.summary.needs_work,
        report.summary.in_progress));
    output.push_str(&format!(" Timestamp: {}\n\n", report.timestamp));

    for feat in &report.features {
        let icon = match feat.status.as_str() {
            "production_ready" => "✅",
            "needs_work" => "⚠️",
            _ => "🔧",
        };
        output.push_str(&format!(" {} {:<20} {:>5.1}% [{}]\n",
            icon, feat.id, feat.overall, feat.status));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maturity::scorer::ScoredFeature;

    #[test]
    fn test_generate_report() {
        let scans = vec![
            ScoredFeature {
                id: "feat-1".to_string(),
                name: "Feature 1".to_string(),
                subcomponents: vec![],
                overall: 95.0,
                status: "production_ready".to_string(),
            },
            ScoredFeature {
                id: "feat-2".to_string(),
                name: "Feature 2".to_string(),
                subcomponents: vec![],
                overall: 40.0,
                status: "in_progress".to_string(),
            },
        ];

        let report = generate_report(scans, 67.5);

        assert_eq!(report.overall_pct, 67.5);
        assert_eq!(report.summary.total_features, 2);
        assert_eq!(report.summary.production_ready, 1);
        assert_eq!(report.summary.in_progress, 1);
        assert_eq!(report.summary.needs_work, 0);
        assert_eq!(report.scanner, "xavier-maturity-engine-v3");
    }

    #[test]
    fn test_format_report() {
        let scans = vec![
            ScoredFeature {
                id: "feat-1".to_string(),
                name: "Feature 1".to_string(),
                subcomponents: vec![],
                overall: 95.0,
                status: "production_ready".to_string(),
            },
        ];
        let report = generate_report(scans, 95.0);
        let formatted = format_report(&report);

        assert!(formatted.contains("Xavier Maturity Report (v3)"));
        assert!(formatted.contains("95.0%"));
        assert!(formatted.contains("feat-1"));
        assert!(formatted.contains("✅"));
    }
}
