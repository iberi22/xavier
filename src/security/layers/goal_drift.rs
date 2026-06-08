//! Goal Drift Detection Layer
//!
//! Detects if the agent's reasoning or actions are deviating from its established goals.

use crate::security::detections::{ScanResult, Severity, Threat, ThreatCategory};

/// Detect goal drift in the given input
pub fn detect_goal_drift(input: &str, result: &mut ScanResult) {
    let drift_patterns = [
        r"(?i)ignore\s+(my|the)\s+original\s+goal",
        r"(?i)new\s+priority\s+is\s+now",
        r"(?i)forget\s+(what\s+i\s+was\s+doing|the\s+previous\s+task)",
        r"(?i)instead\s+of\s+.*,\s+i\s+will\s+now",
        r"(?i)diverging\s+from\s+the\s+assigned\s+mission",
    ];

    for pattern in drift_patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if re.is_match(input) {
                result.add_layer("goal_drift");
                result.clean = false;
                result.threats.push(Threat::new(
                    Severity::Warning,
                    "goal_drift",
                    ThreatCategory::GoalDrift,
                    "Potential agent goal drift detected",
                    pattern,
                    "regex_goal_drift",
                ));
            }
        }
    }
}
