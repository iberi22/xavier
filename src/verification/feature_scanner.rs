//! Feature verifier — portable GitCore feature tracking scanner.
//!
//! Reads `.gitcore/features.json` from the current directory (walking up),
//! validates each feature's implementation paths, test coverage, and staleness,
//! then calculates a weighted "real" progress percentage.
//!
//! Can be used standalone from any GitCore-compliant project root.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ── GitCore Feature JSON Schema ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct FeaturesFile {
    protocol: Option<String>,
    metadata: Option<Metadata>,
    features: Vec<FeatureEntry>,
}

#[derive(Debug, Deserialize)]
struct Metadata {
    project: Option<String>,
    version: Option<String>,
    last_verified: Option<String>,
    total_features: Option<usize>,
    features_complete: Option<usize>,
    overall_progress_pct: Option<f64>,
    passing: Option<usize>,
    failing: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FeatureEntry {
    id: String,
    name: Option<String>,
    category: Option<String>,
    status: Option<String>,
    progress_pct: Option<f64>,
    description: Option<String>,
    steps: Option<Vec<String>>,
    passes: Option<bool>,
    verified_by: Option<String>,
    github_issue: Option<serde_json::Value>,
    last_tested: Option<String>,
    notes: Option<String>,
    implemented_in: Option<String>,
    tests: Option<serde_json::Value>,
}

// ── Scan Results ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct FeatureScanReport {
    pub project: String,
    pub protocol: String,
    pub total_features: usize,
    pub features: Vec<FeatureResult>,
    pub summary: ScanSummary,
}

#[derive(Debug, Serialize)]
pub struct FeatureResult {
    pub id: String,
    pub claimed_pct: f64,
    pub real_pct: f64,
    pub gap: f64,
    pub status: String,
    pub checks: Vec<CheckResult>,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub check: String,
    pub passed: bool,
    pub detail: String,
    pub weight: f64,
}

#[derive(Debug, Serialize)]
pub struct ScanSummary {
    pub total_claimed: f64,
    pub total_real: f64,
    pub avg_gap: f64,
    pub features_ok: usize,
    pub features_with_gap: usize,
    pub features_stale: usize,
    pub features_no_tests: usize,
    pub features_mvp: usize,
}

// ── Core Scanner ───────────────────────────────────────────────────────────

/// Find `.gitcore/features.json` by walking up from `start_dir`.
pub fn find_features_json(start_dir: &Path) -> Result<PathBuf> {
    let mut current = Some(start_dir.to_path_buf());
    while let Some(dir) = current {
        let candidate = dir.join(".gitcore").join("features.json");
        if candidate.exists() {
            return Ok(candidate);
        }
        // Try also just features.json in .gitcore
        let candidate2 = dir.join(".gitcore").join("features.json");
        if candidate2.exists() {
            return Ok(candidate2);
        }
        current = dir.parent().map(|p| p.to_path_buf());
    }
    anyhow::bail!(
        "No .gitcore/features.json found in or above {}",
        start_dir.display()
    );
}

/// Scan a single feature and calculate real progress percentage.
fn scan_feature(feat: &FeatureEntry, root: &Path) -> FeatureResult {
    let mut checks = Vec::new();
    let claimed = feat.progress_pct.unwrap_or(100.0);

    // 1. Check: implemented_in paths exist
    let paths_exist = if let Some(ref imp) = feat.implemented_in {
        if imp == "None" || imp.is_empty() {
            false
        } else {
            let all_exist = imp
                .split(',')
                .map(|p| p.trim().trim_end_matches('/'))
                .filter(|p| !p.is_empty())
                .all(|p| {
                    let full_path = root.join(p);
                    full_path.exists()
                });
            all_exist
        }
    } else {
        false
    };
    checks.push(CheckResult {
        check: "implemented_in paths exist".into(),
        passed: paths_exist,
        detail: if paths_exist {
            format!("paths OK")
        } else {
            format!(
                "missing paths: {}",
                feat.implemented_in.as_deref().unwrap_or("none")
            )
        },
        weight: 0.20,
    });

    // 2. Check: passes field
    let passes = feat.passes.unwrap_or(false);
    checks.push(CheckResult {
        check: "passes = true".into(),
        passed: passes,
        detail: if passes {
            "declared passing".into()
        } else {
            "declared FAILING".into()
        },
        weight: 0.15,
    });

    // 3. Check: tests listed
    let has_tests = feat.tests.is_some()
        && !matches!(feat.tests, Some(serde_json::Value::Array(ref v)) if v.is_empty())
        && !matches!(feat.tests, Some(serde_json::Value::String(ref s)) if s.is_empty() || s == "-");
    checks.push(CheckResult {
        check: "tests listed".into(),
        passed: has_tests,
        detail: if has_tests {
            let count = match &feat.tests {
                Some(serde_json::Value::Array(v)) => format!("{} tests", v.len()),
                Some(serde_json::Value::String(s)) => s.clone(),
                _ => "yes".into(),
            };
            count
        } else {
            "no tests field".into()
        },
        weight: 0.15,
    });

    // 4. Check: last_tested recency
    let is_recent = if let Some(ref lt) = feat.last_tested {
        lt.len() == 10 && lt.as_str() >= "2026-07-01"
    } else {
        false
    };
    checks.push(CheckResult {
        check: "recently tested (since 2026-07-01)".into(),
        passed: is_recent,
        detail: feat
            .last_tested
            .as_deref()
            .unwrap_or("never tested")
            .to_string(),
        weight: 0.15,
    });

    // 5. Check: no MVP/Phase caveats in notes
    let has_caveats = if let Some(ref notes) = feat.notes {
        let lower = notes.to_lowercase();
        lower.contains("mvp")
            || lower.contains("phase 1")
            || lower.contains("optional")
            || lower.contains("polish")
            || lower.contains("missing:")
    } else {
        false
    };
    checks.push(CheckResult {
        check: "no MVP/Phase caveats".into(),
        passed: !has_caveats,
        detail: if has_caveats {
            feat.notes
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(80)
                .collect()
        } else {
            "clean".into()
        },
        weight: 0.15,
    });

    // 6. Check: implemented_in is not null/None
    let has_impl = feat
        .implemented_in
        .as_deref()
        .map(|s| s != "None" && !s.is_empty())
        .unwrap_or(false);
    checks.push(CheckResult {
        check: "implemented_in declared".into(),
        passed: has_impl,
        detail: feat
            .implemented_in
            .as_deref()
            .unwrap_or("missing")
            .to_string(),
        weight: 0.10,
    });

    // 7. Check: status is stable (more weight)
    let is_stable = feat.status.as_deref() == Some("stable");
    checks.push(CheckResult {
        check: "status = stable".into(),
        passed: is_stable,
        detail: feat.status.as_deref().unwrap_or("unknown").to_string(),
        weight: 0.10,
    });

    // Calculate weighted real percentage
    let mut real_pct = 0.0;
    for check in &checks {
        if check.passed {
            real_pct += check.weight * 100.0;
        }
    }
    // Scale: if claimed is 100% but real is lower, report real
    // But real can't exceed claimed
    let real_pct = real_pct.min(claimed);

    FeatureResult {
        id: feat.id.clone(),
        claimed_pct: claimed,
        real_pct: (real_pct * 10.0).round() / 10.0,
        gap: claimed - (real_pct * 10.0).round() / 10.0,
        status: feat.status.clone().unwrap_or_default(),
        checks,
    }
}

/// Run full feature verification scan.
pub fn scan_features(root: Option<&Path>) -> Result<FeatureScanReport> {
    let cwd = root.unwrap_or_else(|| &Path::new("."));
    let features_path = find_features_json(cwd)?;
    let content = std::fs::read_to_string(&features_path)
        .with_context(|| format!("Failed to read {}", features_path.display()))?;

    let parsed: FeaturesFile = serde_json::from_str(&content)
        .with_context(|| format!("Invalid JSON in {}", features_path.display()))?;

    let root_dir = features_path
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(cwd);

    let project = parsed
        .metadata
        .as_ref()
        .and_then(|m| m.project.clone())
        .unwrap_or_else(|| "Unknown".into());

    let protocol = parsed.protocol.unwrap_or_else(|| "unknown".into());

    let mut results = Vec::new();
    for feat in &parsed.features {
        results.push(scan_feature(feat, root_dir));
    }

    let total = results.len() as f64;
    let total_claimed: f64 = results.iter().map(|r| r.claimed_pct).sum();
    let total_real: f64 = results.iter().map(|r| r.real_pct).sum();

    let summary = ScanSummary {
        total_claimed: (total_claimed / total * 10.0).round() / 10.0,
        total_real: (total_real / total * 10.0).round() / 10.0,
        avg_gap: ((total_claimed - total_real) / total * 10.0).round() / 10.0,
        features_ok: results.iter().filter(|r| r.gap < 5.0).count(),
        features_with_gap: results.iter().filter(|r| r.gap >= 5.0).count(),
        features_stale: results
            .iter()
            .filter(|r| {
                r.checks
                    .iter()
                    .any(|c| c.check.contains("recent") && !c.passed)
            })
            .count(),
        features_no_tests: results
            .iter()
            .filter(|r| {
                r.checks
                    .iter()
                    .any(|c| c.check.contains("tests") && !c.passed)
            })
            .count(),
        features_mvp: results
            .iter()
            .filter(|r| {
                r.checks
                    .iter()
                    .any(|c| c.check.contains("caveats") && !c.passed)
            })
            .count(),
    };

    Ok(FeatureScanReport {
        project,
        protocol,
        total_features: results.len(),
        features: results,
        summary,
    })
}

// ── Output Formatting ──────────────────────────────────────────────────────

/// Format scan results as a table string.
pub fn format_report_table(report: &FeatureScanReport) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "Project: {}  |  GitCore: {}  |  Features: {}\n",
        report.project, report.protocol, report.total_features
    ));
    out.push_str(&format!(
        "Claimed: {:.1}%  |  Real: {:.1}%  |  Avg Gap: {:.1}%\n",
        report.summary.total_claimed, report.summary.total_real, report.summary.avg_gap
    ));
    out.push_str(&format!(
        "✅ OK: {}  |  ⚠️ Gap: {}  |  🕰 Stale: {}  |  🧪 No tests: {}  |  🏗 MVP: {}\n",
        report.summary.features_ok,
        report.summary.features_with_gap,
        report.summary.features_stale,
        report.summary.features_no_tests,
        report.summary.features_mvp
    ));
    out.push_str("\n── Feature Reality Audit ─────────────────────────────────────────────\n");
    out.push_str(&format!(
        "{:<32} {:>8} {:>6} {:>6}  {}\n",
        "Feature", "Claimed", "Real", "Gap", "Issues"
    ));
    out.push_str(&"-".repeat(80));
    out.push('\n');

    for feat in &report.features {
        let issues: Vec<&str> = feat
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| {
                let check_name = c.check.as_str();
                if check_name.contains("paths exist") {
                    "no paths"
                } else if check_name.contains("passes") {
                    "failing"
                } else if check_name.contains("tests") {
                    "no tests"
                } else if check_name.contains("recent") {
                    "stale"
                } else if check_name.contains("caveats") {
                    "caveat"
                } else if check_name.contains("implemented_in") {
                    "no impl"
                } else {
                    "?"
                }
            })
            .collect();

        let gap_symbol = if feat.gap > 20.0 {
            "🔴"
        } else if feat.gap > 5.0 {
            "🟡"
        } else {
            "✅"
        };

        out.push_str(&format!(
            "{:<32} {:>6.0}% {:>5.1}% {:>+5.1}  {} {}\n",
            &feat.id[..feat.id.len().min(32)],
            feat.claimed_pct,
            feat.real_pct,
            feat.gap,
            gap_symbol,
            issues.join(", ")
        ));
    }

    out
}

/// Format scan results as JSON.
pub fn format_report_json(report: &FeatureScanReport) -> Result<String> {
    Ok(serde_json::to_string_pretty(report)?)
}

// ── Standalone CLI ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_features_json(dir: &Path, content: &str) -> PathBuf {
        let gitcore = dir.join(".gitcore");
        fs::create_dir_all(&gitcore).unwrap();
        let path = gitcore.join("features.json");
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn find_features_json_walks_up() {
        let tmp = tempdir().unwrap();
        let deep = tmp.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        create_test_features_json(tmp.path(), r#"{"protocol":"3.8.0","features":[]}"#);
        let found = find_features_json(&deep).unwrap();
        assert!(found.ends_with(".gitcore/features.json"));
    }

    #[test]
    fn scan_empty_features_returns_empty_report() {
        let tmp = tempdir().unwrap();
        create_test_features_json(
            tmp.path(),
            r#"{"protocol":"3.8.0","metadata":{"project":"test"},"features":[]}"#,
        );
        let report = scan_features(Some(tmp.path())).unwrap();
        assert_eq!(report.total_features, 0);
        assert_eq!(report.summary.features_ok, 0);
    }

    #[test]
    fn scan_feature_with_all_checks_passing() {
        let tmp = tempdir().unwrap();
        // Create a mock implemented_in path
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("lib.rs"), "pub fn hello() {}").unwrap();

        let json = r#"{
            "protocol": "3.8.0",
            "metadata": {"project": "test", "last_verified": "2026-07-20"},
            "features": [{
                "id": "feat-test",
                "name": "Test Feature",
                "status": "stable",
                "progress_pct": 100,
                "passes": true,
                "verified_by": "automated",
                "last_tested": "2026-07-20",
                "notes": "Fully implemented",
                "implemented_in": "src/lib.rs",
                "tests": ["test_fn1", "test_fn2"]
            }]
        }"#;
        create_test_features_json(tmp.path(), json);
        let report = scan_features(Some(tmp.path())).unwrap();
        assert_eq!(report.total_features, 1);
        let feat = &report.features[0];
        assert_eq!(feat.claimed_pct, 100.0);
        assert!(feat.real_pct > 90.0, "real={}", feat.real_pct);
        assert!(feat.gap < 10.0, "gap={}", feat.gap);
    }

    #[test]
    fn scan_feature_with_caveats_scores_lower() {
        let tmp = tempdir().unwrap();
        let json = r#"{
            "protocol": "3.8.0",
            "metadata": {"project": "test"},
            "features": [{
                "id": "feat-mvp",
                "status": "stable",
                "progress_pct": 100,
                "passes": true,
                "last_tested": "2026-04-01",
                "notes": "MVP complete, Phase 2 pending",
                "implemented_in": "None",
                "tests": []
            }]
        }"#;
        create_test_features_json(tmp.path(), json);
        let report = scan_features(Some(tmp.path())).unwrap();
        let feat = &report.features[0];
        assert_eq!(feat.claimed_pct, 100.0);
        assert!(feat.real_pct < 50.0, "real={} should be <50", feat.real_pct);
        assert!(feat.gap > 50.0, "gap={} should be >50", feat.gap);
    }

    #[test]
    fn format_report_table_contains_headers() {
        let tmp = tempdir().unwrap();
        create_test_features_json(
            tmp.path(),
            r#"{"protocol":"3.8.0","metadata":{"project":"test"},"features":[]}"#,
        );
        let report = scan_features(Some(tmp.path())).unwrap();
        let table = format_report_table(&report);
        assert!(table.contains("Project:"));
        assert!(table.contains("Feature"));
        assert!(table.contains("Claimed"));
    }
}
