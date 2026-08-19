use serde::{Deserialize, Serialize};

/// Simplified Autonomous Remediation Agent (Phase 2 Data Commons).
/// Scans decrypted telemetry logs, classifies anomalies using an LLM,
/// and decides whether to Auto-Fix (PR) or Escalate to Human (Issue).
#[derive(Debug, Serialize, Deserialize)]
pub struct AnomalyReport {
    pub anomaly_type: String,
    pub confidence_score: f32,
    pub proposed_fix: Option<String>,
    pub requires_human: bool,
    pub is_false_positive: bool,
    pub cluster_id: Option<String>,
}

pub struct AnomalyScannerAgent {
    // In production, this holds the LLM Client
    pub name: String,
    pub leak_detector: Option<std::sync::Arc<crate::coordination::secrets::LeakDetector>>,
}

impl AnomalyScannerAgent {
    /// New.
    pub fn new() -> Self {
        Self {
            name: "Xavier-Auto-Healer".to_string(),
            leak_detector: None,
        }
    }

    /// With leak detector.
    pub fn with_leak_detector(
        mut self,
        leak_detector: std::sync::Arc<crate::coordination::secrets::LeakDetector>,
    ) -> Self {
        self.leak_detector = Some(leak_detector);
        self
    }

    /// Evaluates a raw telemetry message and returns an AnomalyReport.
    pub async fn scan_telemetry(&self, telemetry_json: &str) -> AnomalyReport {
        // MOCK: In production, we call LLM (Gemini/Claude) with the telemetry_json

        // 1. Leak Detection
        if let Some(ref detector) = self.leak_detector {
            if let Some((agent_id, _hash)) = detector.check_leak(telemetry_json).await {
                return AnomalyReport {
                    anomaly_type: "API Key Leak Detected".to_string(),
                    confidence_score: 1.0,
                    proposed_fix: Some(format!("Revoke all leases for agent {}", agent_id)),
                    requires_human: true,
                    is_false_positive: false,
                    cluster_id: Some("CLUSTER_SECURITY_LEAK".to_string()),
                };
            }
        }

        let is_rust_panic = telemetry_json.contains("panic") || telemetry_json.contains("Crash");
        let is_p2p_race =
            telemetry_json.contains("PeerJS") || telemetry_json.contains("YJS_UPDATE");

        let is_false_positive = self.evaluate_false_positive(telemetry_json);
        if is_false_positive {
            return AnomalyReport {
                anomaly_type: "Ignored (False Positive)".to_string(),
                confidence_score: 0.99,
                proposed_fix: None,
                requires_human: false,
                is_false_positive: true,
                cluster_id: None,
            };
        }

        let cluster_id = self.cluster_anomalies(telemetry_json);

        if is_rust_panic {
            // High confidence, we might not auto-fix core rust panics easily
            AnomalyReport {
                anomaly_type: "Rust Core Panic".to_string(),
                confidence_score: 0.95,
                proposed_fix: None,
                requires_human: true, // Escalate to Jules
                is_false_positive: false,
                cluster_id: Some(cluster_id),
            }
        } else if is_p2p_race {
            // We know how to fix P2P race conditions using exponential backoff
            AnomalyReport {
                anomaly_type: "P2P Sync Race Condition".to_string(),
                confidence_score: 0.88,
                proposed_fix: Some(
                    "Implement exponential backoff in p2p.ts and debounce Yjs updates".to_string(),
                ),
                requires_human: false, // Xavier can Auto-Fix this
                is_false_positive: false,
                cluster_id: Some(cluster_id),
            }
        } else {
            // Unknown anomaly
            AnomalyReport {
                anomaly_type: "Unknown Error".to_string(),
                confidence_score: 0.40,
                proposed_fix: None,
                requires_human: true,
                is_false_positive: false,
                cluster_id: Some(cluster_id),
            }
        }
    }

    /// Triage logic: Filters out deterministic noise using CodeGraph heuristics.
    fn evaluate_false_positive(&self, telemetry_json: &str) -> bool {
        // MOCK: If the error is a known harmless warning, drop it automatically.
        telemetry_json.contains("User aborted")
            || telemetry_json.contains("Connection reset by peer")
    }

    /// Clustering logic: Groups similar issues using local vector embeddings.
    fn cluster_anomalies(&self, telemetry_json: &str) -> String {
        // MOCK: In production, generate an embedding of telemetry_json and query Qdrant/pgvector.
        if telemetry_json.contains("PeerJS") || telemetry_json.contains("YJS_UPDATE") {
            "CLUSTER_P2P_NETWORK".to_string()
        } else if telemetry_json.contains("panic") {
            "CLUSTER_CORE_RUST".to_string()
        } else {
            "CLUSTER_MISC".to_string()
        }
    }

    /// Simulates taking action based on the AnomalyReport
    pub fn execute_remediation(&self, report: &AnomalyReport) -> String {
        if report.is_false_positive {
            "Closing anomaly: Detected as False Positive by Triage".to_string()
        } else if report.requires_human {
            format!(
                "Grouping anomaly into Epics (Cluster: {:?}). Waiting for DAO Governance Vote.",
                report.cluster_id
            )
        } else if let Some(fix) = &report.proposed_fix {
            format!(
                "Auto-Fixing: Generating PR for Cluster {:?} to apply fix: {}",
                report.cluster_id, fix
            )
        } else {
            "No action taken".to_string()
        }
    }
}

impl Default for AnomalyScannerAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_anomaly_scanner_human_escalation() {
        let agent = AnomalyScannerAgent::new();
        let report = agent
            .scan_telemetry(
                "{\"event_kind\": \"panic\", \"sanitized_message\": \"Crash in [REDACTED]\"}",
            )
            .await;

        assert_eq!(report.anomaly_type, "Rust Core Panic");
        assert!(report.requires_human);

        let action = agent.execute_remediation(&report);
        assert!(action.contains("DAO Governance Vote"));
    }

    #[tokio::test]
    async fn test_anomaly_scanner_auto_fix() {
        let agent = AnomalyScannerAgent::new();
        let report = agent.scan_telemetry("{\"event_kind\": \"sync_error\", \"sanitized_message\": \"PeerJS connection failed, YJS_UPDATE race\"}").await;

        assert_eq!(report.anomaly_type, "P2P Sync Race Condition");
        assert!(!report.requires_human);

        let action = agent.execute_remediation(&report);
        assert!(action.contains("Auto-Fixing"));
        assert!(action.contains("exponential backoff"));
    }

    #[tokio::test]
    async fn test_anomaly_scanner_leak_detection() {
        let detector = std::sync::Arc::new(crate::coordination::secrets::LeakDetector::new());
        let secret = "leaked-secret-key";
        let agent_id = "agent-123";
        detector.register_key(secret, agent_id).await;

        let agent = AnomalyScannerAgent::new().with_leak_detector(detector);
        let telemetry = format!("{{\"log\": \"Error sending request with key {}\"}}", secret);

        let report = agent.scan_telemetry(&telemetry).await;

        assert_eq!(report.anomaly_type, "API Key Leak Detected");
        assert!(report.requires_human);
        assert!(report.proposed_fix.unwrap().contains(agent_id));
    }
}
